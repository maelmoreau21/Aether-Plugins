use crate::adjust::{overlap_ratio, snap_segment};
use crate::chapters::{ChapterClassifier, NAMED_CHAPTER_CONFIDENCE, chapter_boundaries};
use crate::credits::CreditsDetector;
use crate::matcher::{FingerprintMatcher, normalize_rate};
use crate::types::{
    DetectionMethod, EpisodeData, EpisodeDetectionResult, IntroMarker, IntroSkipperConfig,
    MarkerType,
};
use std::collections::HashMap;

/// Confidence of a segment found by the audio and confirmed by the file's chapters.
pub const CONFIRMED_CONFIDENCE: f64 = 0.99;

/// Acoustic consensus of one episode for one kind: (start, end, confidence).
type Consensus = HashMap<String, (f64, f64, f64)>;

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
    ///    a chapter guessed from its shape is kept when the audio agrees, otherwise the audio
    ///    wins; an acoustic boundary close to a chapter mark moves onto it.
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
        let intros = if acoustic && self.config.detect_intro {
            self.detect_fingerprint_intros(episodes)
        } else {
            HashMap::new()
        };
        let credits = if acoustic && self.config.detect_credits {
            self.detect_fingerprint_credits(episodes)
        } else {
            HashMap::new()
        };

        let classifier = ChapterClassifier::new(self.config);
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
                for (kind, found, analysed_end) in [
                    (MarkerType::Intro, intros.get(&ep.id), head_end),
                    (MarkerType::Credits, credits.get(&ep.id), None),
                ] {
                    let Some(&(start, end, confidence)) = found else {
                        continue;
                    };
                    let (start, end) = snap_segment(
                        kind,
                        start,
                        end,
                        ep.duration,
                        &boundaries,
                        self.config.chapter_snap_window,
                        analysed_end,
                    );
                    fuse(&mut markers, &ep.id, kind, (start, end), confidence);
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
        let fp_episodes: Vec<&EpisodeData> = episodes
            .iter()
            .filter(|e| e.fingerprints.as_ref().is_some_and(|f| !f.is_empty()))
            .collect();

        let mut candidates_by_ep: HashMap<String, Vec<(f64, f64, f64)>> = HashMap::new();
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
                candidates_by_ep.entry(ep_a.id.clone()).or_default().push((
                    m.offset_a,
                    m.offset_a + m.duration,
                    m.score,
                ));
                candidates_by_ep.entry(ep_b.id.clone()).or_default().push((
                    m.offset_b,
                    m.offset_b + m.duration,
                    m.score,
                ));
            }
        }
        self.consensus(candidates_by_ep, fp_episodes.len())
    }

    /// Run credits detection on episode tails with multi-episode consensus
    fn detect_fingerprint_credits(&self, episodes: &[EpisodeData]) -> Consensus {
        let detector = CreditsDetector::new(self.config);
        let fp_episodes: Vec<&EpisodeData> = episodes
            .iter()
            .filter(|e| e.tail_fingerprints.as_ref().is_some_and(|f| !f.is_empty()))
            .collect();

        let mut candidates_by_ep: HashMap<String, Vec<(f64, f64, f64)>> = HashMap::new();
        for (idx_a, idx_b) in pairs(fp_episodes.len()) {
            let ep_a = fp_episodes[idx_a];
            let ep_b = fp_episodes[idx_b];
            if let Some((marker_a, marker_b)) = detector.detect_tail_credits(ep_a, ep_b)
                && marker_a.confidence >= self.config.similarity_threshold
            {
                candidates_by_ep.entry(ep_a.id.clone()).or_default().push((
                    marker_a.start,
                    marker_a.end,
                    marker_a.confidence,
                ));
                candidates_by_ep.entry(ep_b.id.clone()).or_default().push((
                    marker_b.start,
                    marker_b.end,
                    marker_b.confidence,
                ));
            }
        }
        self.consensus(candidates_by_ep, fp_episodes.len())
    }

    /// The best agreeing cluster of each episode, if it has enough votes.
    fn consensus(
        &self,
        candidates_by_ep: HashMap<String, Vec<(f64, f64, f64)>>,
        fp_count: usize,
    ) -> Consensus {
        candidates_by_ep
            .into_iter()
            .filter_map(|(ep_id, matches)| {
                let (start, end, confidence, votes) = cluster_candidates(&matches)?;
                // Uncertainty guard: if only 1 match among 3+ fingerprinted episodes and confidence is weak, reject
                if (votes as u32) < self.config.min_consensus_votes
                    || (votes == 1 && fp_count >= 3 && confidence < 0.80)
                {
                    return None;
                }
                Some((ep_id, (start, end, confidence)))
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
        chapter.start = audio.0;
        chapter.end = audio.1;
        chapter.confidence = confidence;
        chapter.method = DetectionMethod::Fingerprint;
        chapter.id = format!("{episode_id}-{kind}-fp");
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

/// Cluster candidate segments by start offset and duration proximity
fn cluster_candidates(matches: &[(f64, f64, f64)]) -> Option<(f64, f64, f64, usize)> {
    if matches.is_empty() {
        return None;
    }

    struct Cluster {
        starts: Vec<f64>,
        ends: Vec<f64>,
        scores: Vec<f64>,
    }

    let mut clusters: Vec<Cluster> = Vec::new();

    for &(start, end, score) in matches {
        let dur = end - start;
        let mut added = false;
        for c in &mut clusters {
            let avg_start = c.starts.iter().sum::<f64>() / c.starts.len() as f64;
            let avg_dur =
                (c.ends.iter().sum::<f64>() - c.starts.iter().sum::<f64>()) / c.starts.len() as f64;
            if (start - avg_start).abs() <= 2.5 && (dur - avg_dur).abs() <= 2.5 {
                c.starts.push(start);
                c.ends.push(end);
                c.scores.push(score);
                added = true;
                break;
            }
        }
        if !added {
            clusters.push(Cluster {
                starts: vec![start],
                ends: vec![end],
                scores: vec![score],
            });
        }
    }

    // Select the cluster with maximum votes, then highest average score
    clusters
        .into_iter()
        .max_by(|a, b| {
            a.starts.len().cmp(&b.starts.len()).then_with(|| {
                let sa = a.scores.iter().sum::<f64>() / a.scores.len() as f64;
                let sb = b.scores.iter().sum::<f64>() / b.scores.len() as f64;
                sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal)
            })
        })
        .map(|c| {
            let count = c.starts.len();
            let avg_start = c.starts.iter().sum::<f64>() / count as f64;
            let avg_end = c.ends.iter().sum::<f64>() / count as f64;
            let avg_score = c.scores.iter().sum::<f64>() / count as f64;
            // Consensus boost: 2+ confirming episodes elevate confidence
            let confidence = if count >= 2 {
                (avg_score + 0.08 * (count.min(4) - 1) as f64).min(0.99)
            } else {
                avg_score
            };
            (avg_start, avg_end, confidence, count)
        })
}
