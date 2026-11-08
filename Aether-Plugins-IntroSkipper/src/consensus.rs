use crate::adjust::{overlap_ratio, snap_segment_in};
use crate::chapters::{
    ChapterClassifier, NAMED_CHAPTER_CONFIDENCE, STRUCTURE_UNHEARD_CONFIDENCE, chapter_boundaries,
    is_structural,
};
use crate::credits::CreditsDetector;
use crate::matcher::{FingerprintMatcher, normalize_rate};
use crate::types::{
    DetectionMethod, EpisodeData, EpisodeDetectionResult, IntroMarker, IntroSkipperConfig,
    MarkerType,
};
use std::collections::HashMap;

/// Confidence of a segment found by the audio and confirmed by the file's chapters.
pub const CONFIRMED_CONFIDENCE: f64 = 0.99;

/// A match closer than this to the edge of the analysed audio may have been cut there.
const EDGE_MARGIN: f64 = 2.0;

/// One acoustic segment of one episode: the match of one comparison, or the agreement of
/// several.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    start: f64,
    end: f64,
    confidence: f64,
    /// The match began at the start edge of the analysed audio of one of the episodes compared.
    cut_start: bool,
    /// The match stopped at the end edge of the analysed audio of one of the episodes compared.
    cut_end: bool,
}

/// Acoustic consensus of one episode for one kind.
type Consensus = HashMap<String, Segment>;

fn has_head(ep: &EpisodeData) -> bool {
    ep.fingerprints.as_ref().is_some_and(|f| !f.is_empty())
}

fn has_tail(ep: &EpisodeData) -> bool {
    ep.tail_fingerprints.as_ref().is_some_and(|f| !f.is_empty())
}

/// Season-wide analysis engine delivering multi-episode consensus
pub struct SeasonAnalyzer<'a> {
    config: &'a IntroSkipperConfig,
}

impl<'a> SeasonAnalyzer<'a> {
    pub fn new(config: &'a IntroSkipperConfig) -> Self {
        Self { config }
    }

    /// Analyze a list of episodes belonging to the same season / show.
    ///
    /// 1. Chapters: named chapters, or the shape of unnamed ones (see [`ChapterClassifier`]).
    /// 2. Audio: openings and credits shared by the episodes of the batch.
    /// 3. Fusion: a named chapter is kept as authored (audio agreement raises it to 0.99);
    ///    a chapter guessed from its shape is applied only when the audio agrees, replaced when
    ///    the audio finds something else, and lowered when the audio was compared and found
    ///    nothing; an acoustic boundary close to a chapter mark moves onto it.
    pub fn analyze_season(&self, episodes: &[EpisodeData]) -> Vec<EpisodeDetectionResult> {
        // The host must supply one explicit series/season scope. Never compare
        // unrelated episodes even if a caller accidentally mixes its batch.
        let scope = episodes.first().map(|ep| (&ep.series_id, ep.season_number));
        if episodes
            .iter()
            .any(|ep| Some((&ep.series_id, ep.season_number)) != scope)
        {
            return episodes
                .iter()
                .map(|ep| EpisodeDetectionResult {
                    episode_id: ep.id.clone(),
                    markers: Vec::new(),
                    intro: None,
                    credits: None,
                })
                .collect();
        }

        let acoustic = self.config.enable_fingerprint_detection
            && episodes.len() >= 2
            && episodes.first().is_some_and(|ep| {
                ep.series_id.as_ref().is_some_and(|id| !id.is_empty()) && ep.season_number.is_some()
            });
        let listen_intro = acoustic && self.config.detect_intro;
        let listen_credits = acoustic && self.config.detect_credits;
        let intros = if listen_intro {
            self.detect_fingerprint_intros(episodes)
        } else {
            HashMap::new()
        };
        let credits = if listen_credits {
            self.detect_fingerprint_credits(episodes)
        } else {
            HashMap::new()
        };
        let heads = episodes.iter().filter(|ep| has_head(ep)).count();
        let tails = episodes.iter().filter(|ep| has_tail(ep)).count();

        let classifier = ChapterClassifier::new(self.config);
        let detector = CreditsDetector::new(self.config);
        episodes
            .iter()
            .map(|ep| {
                let chapters = ep.chapters.as_deref().unwrap_or_default();
                let mut markers = if self.config.enable_chapter_detection {
                    classifier.parse_markers(&ep.id, chapters, ep.duration)
                } else {
                    Vec::new()
                };
                let boundaries = chapter_boundaries(chapters, ep.duration);
                let rate = normalize_rate(ep.fingerprint_rate);
                let head_end = ep
                    .fingerprints
                    .as_ref()
                    .map(|fps| self.config.search_window_end.min(fps.len() as f64 / rate));
                let tail_begin = detector.analysed_tail_start(ep);
                for (kind, found, heard) in [
                    (
                        MarkerType::Intro,
                        intros.get(&ep.id),
                        listen_intro && has_head(ep) && heads >= 2,
                    ),
                    (
                        MarkerType::Credits,
                        credits.get(&ep.id),
                        listen_credits && has_tail(ep) && tails >= 2,
                    ),
                ] {
                    let Some(found) = found else {
                        // The audio of this episode was compared with its peers and shares
                        // nothing there: a guess from the shape of the chapters is likely story
                        // (a cold open, a final scene).
                        if heard {
                            for guess in markers
                                .iter_mut()
                                .filter(|m| m.marker_type == kind && is_structural(m))
                            {
                                guess.confidence =
                                    guess.confidence.min(STRUCTURE_UNHEARD_CONFIDENCE);
                            }
                        }
                        continue;
                    };
                    // A match cut at the edge of this episode's audio, or of the peer's, may
                    // continue beyond it: the match's own boundary then stands for the edge.
                    let analysed = match kind {
                        MarkerType::Intro => (
                            None,
                            if found.cut_end {
                                Some(found.end)
                            } else {
                                head_end
                            },
                        ),
                        _ => (
                            if found.cut_start {
                                Some(found.start)
                            } else {
                                tail_begin
                            },
                            None,
                        ),
                    };
                    let (start, end) = snap_segment_in(
                        kind,
                        found.start,
                        found.end,
                        ep.duration,
                        &boundaries,
                        self.config.chapter_snap_window,
                        analysed,
                    );
                    fuse(&mut markers, &ep.id, kind, (start, end), found.confidence);
                }
                markers.sort_by(|a, b| a.start.total_cmp(&b.start));
                let markers = self.config.finalize(markers, ep.duration);
                EpisodeDetectionResult {
                    episode_id: ep.id.clone(),
                    intro: markers
                        .iter()
                        .find(|m| m.marker_type == MarkerType::Intro)
                        .cloned(),
                    credits: markers
                        .iter()
                        .find(|m| m.marker_type == MarkerType::Credits)
                        .cloned(),
                    markers,
                }
            })
            .collect()
    }

    /// Run acoustic fingerprint matching between consecutive and anchor episodes
    fn detect_fingerprint_intros(&self, episodes: &[EpisodeData]) -> Consensus {
        // Collect episodes having fingerprint data
        let fp_episodes: Vec<&EpisodeData> = episodes.iter().filter(|e| has_head(e)).collect();

        let mut candidates_by_ep: HashMap<String, Vec<Segment>> = HashMap::new();
        for (idx_a, idx_b) in pairs(fp_episodes.len()) {
            let ep_a = fp_episodes[idx_a];
            let ep_b = fp_episodes[idx_b];
            if let (Some(a), Some(b)) = (ep_a.fingerprint_rate, ep_b.fingerprint_rate)
                && (normalize_rate(Some(a)) - normalize_rate(Some(b))).abs() > 0.01
            {
                continue;
            }

            let fps_a = ep_a.fingerprints.as_ref().unwrap();
            let fps_b = ep_b.fingerprints.as_ref().unwrap();
            let rate = ep_a.fingerprint_rate.or(ep_b.fingerprint_rate);

            let matcher = FingerprintMatcher::new(self.config, rate);
            if let Some(m) = matcher.find_best_match(fps_a, fps_b)
                && m.duration >= self.config.min_intro_duration
                && m.duration <= self.config.max_intro_duration
                && m.score >= self.config.similarity_threshold
            {
                // Same rate as the matcher: where each head stops being searched.
                let rate = normalize_rate(rate).max(1.0);
                let edge = |fps: &[u32]| self.config.search_window_end.min(fps.len() as f64 / rate);
                let cut_end = m.offset_a + m.duration >= edge(fps_a) - EDGE_MARGIN
                    || m.offset_b + m.duration >= edge(fps_b) - EDGE_MARGIN;
                for (id, offset) in [(&ep_a.id, m.offset_a), (&ep_b.id, m.offset_b)] {
                    candidates_by_ep
                        .entry(id.clone())
                        .or_default()
                        .push(Segment {
                            start: offset,
                            end: offset + m.duration,
                            confidence: m.score,
                            cut_start: false,
                            cut_end,
                        });
                }
            }
        }
        self.consensus(candidates_by_ep, &fp_episodes)
    }

    /// Run credits detection on episode tails with multi-episode consensus
    fn detect_fingerprint_credits(&self, episodes: &[EpisodeData]) -> Consensus {
        let detector = CreditsDetector::new(self.config);
        let fp_episodes: Vec<&EpisodeData> = episodes.iter().filter(|e| has_tail(e)).collect();

        let mut candidates_by_ep: HashMap<String, Vec<Segment>> = HashMap::new();
        for (idx_a, idx_b) in pairs(fp_episodes.len()) {
            let ep_a = fp_episodes[idx_a];
            let ep_b = fp_episodes[idx_b];
            if let Some((marker_a, marker_b)) = detector.detect_tail_credits(ep_a, ep_b)
                && marker_a.confidence >= self.config.similarity_threshold
            {
                let at_edge = |ep: &EpisodeData, start: f64| {
                    detector
                        .analysed_tail_start(ep)
                        .is_some_and(|edge| start <= edge + EDGE_MARGIN)
                };
                let cut_start = at_edge(ep_a, marker_a.start) || at_edge(ep_b, marker_b.start);
                for (id, marker) in [(&ep_a.id, marker_a), (&ep_b.id, marker_b)] {
                    candidates_by_ep
                        .entry(id.clone())
                        .or_default()
                        .push(Segment {
                            start: marker.start,
                            end: marker.end,
                            confidence: marker.confidence,
                            cut_start,
                            cut_end: false,
                        });
                }
            }
        }
        self.consensus(candidates_by_ep, &fp_episodes)
    }

    /// The best agreeing cluster of each episode, if it has enough votes.
    fn consensus(
        &self,
        candidates_by_ep: HashMap<String, Vec<Segment>>,
        fp_episodes: &[&EpisodeData],
    ) -> Consensus {
        let fp_count = fp_episodes.len();
        // The most votes an episode can get: one per comparison it takes part in.
        let mut possible: HashMap<&str, u32> = HashMap::new();
        for (a, b) in pairs(fp_count) {
            *possible.entry(fp_episodes[a].id.as_str()).or_default() += 1;
            *possible.entry(fp_episodes[b].id.as_str()).or_default() += 1;
        }
        candidates_by_ep
            .into_iter()
            .filter_map(|(ep_id, matches)| {
                let (segment, votes) = cluster_candidates(&matches)?;
                // The Core sends three episodes at most, so an episode is compared twice at
                // most: a requirement above what the batch allows would silently drop every
                // acoustic segment. "2" still needs three episodes, as the setting says.
                let reachable = possible.get(ep_id.as_str()).copied().unwrap_or(0).max(2);
                let required = self.config.min_consensus_votes.min(reachable);
                // Uncertainty guard: a single weak match among 3+ fingerprinted episodes is
                // rejected when another comparison found something else for this episode.
                let contradicted = matches.len() > votes;
                if (votes as u32) < required
                    || (votes == 1 && fp_count >= 3 && contradicted && segment.confidence < 0.80)
                {
                    return None;
                }
                Some((ep_id, segment))
            })
            .collect()
    }
}

/// Comparison pairs: consecutive (i, i+1), lookahead (i, i+2) and, in small batches, (i, i+3).
fn pairs(count: usize) -> Vec<(usize, usize)> {
    let mut pair_indices = Vec::new();
    for i in 0..count.saturating_sub(1) {
        pair_indices.push((i, i + 1));
        if i + 2 < count {
            pair_indices.push((i, i + 2));
        }
        if i + 3 < count && count <= 8 {
            pair_indices.push((i, i + 3));
        }
    }
    pair_indices
}

/// Merge one acoustic segment into the chapter markers of an episode.
fn fuse(
    markers: &mut Vec<IntroMarker>,
    episode_id: &str,
    kind: MarkerType,
    audio: (f64, f64),
    confidence: f64,
) {
    if let Some(chapter) = markers.iter_mut().find(|m| m.marker_type == kind) {
        let agreement = overlap_ratio((chapter.start, chapter.end), audio);
        if chapter.confidence >= NAMED_CHAPTER_CONFIDENCE {
            // A chapter named "Opening" is the author's cut: the audio only confirms it.
            if agreement > 0.0 {
                chapter.confidence = CONFIRMED_CONFIDENCE;
            }
            return;
        }
        if agreement >= 0.5 {
            chapter.confidence = CONFIRMED_CONFIDENCE;
            return;
        }
        // The chapter's shape was a guess and the audio says otherwise.
        let replaced_guess = is_structural(chapter);
        chapter.start = audio.0;
        chapter.end = audio.1;
        chapter.confidence = confidence;
        chapter.method = DetectionMethod::Fingerprint;
        chapter.id = format!("{episode_id}-{kind}-fp");
        // The preview was guessed as the chapter after that ending: it goes with it.
        if replaced_guess && kind == MarkerType::Credits {
            markers.retain(|m| !(m.marker_type == MarkerType::Preview && is_structural(m)));
        }
        return;
    }
    markers.push(IntroMarker {
        id: format!("{episode_id}-{kind}-fp"),
        start: audio.0,
        end: audio.1,
        marker_type: kind,
        confidence,
        method: DetectionMethod::Fingerprint,
    });
}

/// Cluster candidate segments by start offset. Comparisons often agree on where a segment
/// starts but not on where it stops (a shorter opening in one peer, a match cut at the edge of
/// the analysed audio): the longest match of a cluster gives its end.
fn cluster_candidates(matches: &[Segment]) -> Option<(Segment, usize)> {
    let mean = |values: &mut dyn Iterator<Item = f64>| {
        let (sum, count) = values.fold((0.0, 0usize), |(s, n), v| (s + v, n + 1));
        sum / count.max(1) as f64
    };
    let mut clusters: Vec<Vec<Segment>> = Vec::new();
    for &candidate in matches {
        let near = clusters.iter_mut().find(|cluster| {
            (candidate.start - mean(&mut cluster.iter().map(|m| m.start))).abs() <= 2.5
        });
        match near {
            Some(cluster) => cluster.push(candidate),
            None => clusters.push(vec![candidate]),
        }
    }

    // Select the cluster with maximum votes, then highest average score
    clusters
        .into_iter()
        .max_by(|a, b| {
            a.len().cmp(&b.len()).then_with(|| {
                let sa = mean(&mut a.iter().map(|m| m.confidence));
                let sb = mean(&mut b.iter().map(|m| m.confidence));
                sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .map(|cluster| {
            let count = cluster.len();
            let longest = cluster
                .iter()
                .copied()
                .max_by(|a, b| a.end.total_cmp(&b.end))
                .expect("a cluster holds at least one match");
            let avg_score = mean(&mut cluster.iter().map(|m| m.confidence));
            // Consensus boost: 2+ confirming episodes elevate confidence
            let confidence = if count >= 2 {
                (avg_score + 0.08 * (count.min(4) - 1) as f64).min(0.99)
            } else {
                avg_score
            };
            let segment = Segment {
                start: mean(&mut cluster.iter().map(|m| m.start)),
                end: longest.end,
                confidence,
                cut_start: cluster.iter().any(|m| m.cut_start),
                cut_end: longest.cut_end,
            };
            (segment, count)
        })
}
