use crate::chapters::ChapterClassifier;
use crate::credits::CreditsDetector;
use crate::matcher::FingerprintMatcher;
use crate::types::{
    DetectionMethod, EpisodeData, EpisodeDetectionResult, IntroMarker, IntroSkipperConfig,
    MarkerType,
};
use std::collections::HashMap;

/// Season-wide analysis engine delivering multi-episode consensus
pub struct SeasonAnalyzer<'a> {
    config: &'a IntroSkipperConfig,
}

impl<'a> SeasonAnalyzer<'a> {
    pub fn new(config: &'a IntroSkipperConfig) -> Self {
        Self { config }
    }

    /// Analyze a list of episodes belonging to the same season / show
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
        let mut results_map: HashMap<String, EpisodeDetectionResult> = HashMap::new();

        // Initialize empty results
        for ep in episodes {
            results_map.insert(
                ep.id.clone(),
                EpisodeDetectionResult {
                    episode_id: ep.id.clone(),
                    markers: Vec::new(),
                    intro: None,
                    credits: None,
                },
            );
        }

        // Pass 1: Chapter-based detection (instantaneous, high precision)
        if self.config.enable_chapter_detection {
            let classifier = ChapterClassifier::new(self.config);
            for ep in episodes {
                if let Some(chapters) = &ep.chapters {
                    let markers = classifier.parse_markers(&ep.id, chapters, ep.duration);
                    if let Some(res) = results_map.get_mut(&ep.id) {
                        for marker in markers {
                            if marker.marker_type == MarkerType::Intro && res.intro.is_none() {
                                res.intro = Some(marker.clone());
                            } else if marker.marker_type == MarkerType::Credits
                                && res.credits.is_none()
                            {
                                res.credits = Some(marker.clone());
                            }
                            res.markers.push(marker);
                        }
                    }
                }
            }
        }

        // Pass 2: Fingerprint-based detection with cross-episode consensus
        if self.config.enable_fingerprint_detection
            && episodes.len() >= 2
            && episodes.first().is_some_and(|ep| {
                ep.series_id.as_ref().is_some_and(|id| !id.is_empty()) && ep.season_number.is_some()
            })
        {
            self.detect_fingerprint_intros(episodes, &mut results_map);
            self.detect_fingerprint_credits(episodes, &mut results_map);
        }

        // Build ordered output matching input list
        episodes
            .iter()
            .filter_map(|ep| results_map.remove(&ep.id))
            .collect()
    }

    /// Run acoustic fingerprint matching between consecutive and anchor episodes
    fn detect_fingerprint_intros(
        &self,
        episodes: &[EpisodeData],
        results: &mut HashMap<String, EpisodeDetectionResult>,
    ) {
        // Collect episodes having fingerprint data
        let fp_episodes: Vec<&EpisodeData> = episodes
            .iter()
            .filter(|e| e.fingerprints.as_ref().is_some_and(|f| !f.is_empty()))
            .collect();

        if fp_episodes.len() < 2 {
            return;
        }

        // Build comparison pairs: consecutive (i, i+1), lookahead (i, i+2) and anchor pairs
        let mut pair_indices: Vec<(usize, usize)> = Vec::new();
        for i in 0..(fp_episodes.len() - 1) {
            pair_indices.push((i, i + 1));
            if i + 2 < fp_episodes.len() {
                pair_indices.push((i, i + 2));
            }
            if i + 3 < fp_episodes.len() && fp_episodes.len() <= 8 {
                pair_indices.push((i, i + 3));
            }
        }

        // Discovered candidate matches: ep_id -> Vec<(start, end, score)>
        let mut candidates_by_ep: HashMap<String, Vec<(f64, f64, f64)>> = HashMap::new();

        for (idx_a, idx_b) in pair_indices {
            let ep_a = fp_episodes[idx_a];
            let ep_b = fp_episodes[idx_b];
            if let (Some(a), Some(b)) = (ep_a.fingerprint_rate, ep_b.fingerprint_rate)
                && (a - b).abs() > 0.01
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

        let fp_count = fp_episodes.len();

        // Apply multi-episode consensus clustering for each episode
        for (ep_id, matches) in candidates_by_ep {
            if matches.is_empty() {
                continue;
            }

            // Cluster candidate matches by similar start time and duration
            let best_cluster = cluster_candidates(&matches);
            let Some((start, end, confidence, votes)) = best_cluster else {
                continue;
            };

            // Uncertainty guard: if only 1 match among 3+ fingerprinted episodes and confidence is weak, reject
            if votes == 1 && fp_count >= 3 && confidence < 0.80 {
                continue;
            }

            if let Some(res) = results.get_mut(&ep_id) {
                // If a chapter intro already exists, check if audio confirms and refines it
                if let Some(ref mut chap_intro) = res.intro {
                    if chap_intro.method == DetectionMethod::Chapter {
                        // Check if audio overlap with chapter exists
                        let overlaps = start < chap_intro.end && end > chap_intro.start;
                        if overlaps {
                            // Audio fingerprint refines exact timing and maximizes confidence
                            chap_intro.start = start;
                            chap_intro.end = end;
                            chap_intro.confidence = 0.99;
                            chap_intro.method = DetectionMethod::Fingerprint;

                            // Update in markers array
                            for m in res.markers.iter_mut() {
                                if m.marker_type == MarkerType::Intro
                                    && m.method == DetectionMethod::Chapter
                                {
                                    m.start = start;
                                    m.end = end;
                                    m.confidence = 0.99;
                                    m.method = DetectionMethod::Fingerprint;
                                }
                            }
                        }
                    }
                    continue;
                }

                let marker = IntroMarker {
                    id: format!("{ep_id}-intro-fp"),
                    start,
                    end,
                    marker_type: MarkerType::Intro,
                    confidence,
                    method: DetectionMethod::Fingerprint,
                };
                res.intro = Some(marker.clone());
                res.markers.push(marker);
            }
        }
    }

    /// Run credits detection on episode tails with multi-episode consensus
    fn detect_fingerprint_credits(
        &self,
        episodes: &[EpisodeData],
        results: &mut HashMap<String, EpisodeDetectionResult>,
    ) {
        let detector = CreditsDetector::new(self.config);
        let fp_episodes: Vec<&EpisodeData> = episodes
            .iter()
            .filter(|e| {
                e.tail_fingerprints.as_ref().is_some_and(|f| !f.is_empty())
                    || (e.fingerprints.as_ref().is_some_and(|f| !f.is_empty()) && e.duration > 0.0)
            })
            .collect();

        if fp_episodes.len() < 2 {
            return;
        }

        let mut pair_indices: Vec<(usize, usize)> = Vec::new();
        for i in 0..(fp_episodes.len().saturating_sub(1)) {
            pair_indices.push((i, i + 1));
            if i + 2 < fp_episodes.len() {
                pair_indices.push((i, i + 2));
            }
            if i + 3 < fp_episodes.len() && fp_episodes.len() <= 8 {
                pair_indices.push((i, i + 3));
            }
        }

        let mut candidates_by_ep: HashMap<String, Vec<(f64, f64, f64)>> = HashMap::new();

        for (idx_a, idx_b) in pair_indices {
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

        let fp_count = fp_episodes.len();

        for (ep_id, matches) in candidates_by_ep {
            if matches.is_empty() {
                continue;
            }

            let best_cluster = cluster_candidates(&matches);
            let Some((start, end, confidence, votes)) = best_cluster else {
                continue;
            };

            if votes == 1 && fp_count >= 3 && confidence < 0.80 {
                continue;
            }

            if let Some(res) = results.get_mut(&ep_id) {
                // If chapter credits already exist, check overlap
                if let Some(ref mut chap_credits) = res.credits {
                    if chap_credits.method == DetectionMethod::Chapter {
                        let overlaps = start < chap_credits.end && end > chap_credits.start;
                        if overlaps {
                            chap_credits.start = start;
                            chap_credits.end = end;
                            chap_credits.confidence = 0.99;
                            chap_credits.method = DetectionMethod::Fingerprint;

                            for m in res.markers.iter_mut() {
                                if m.marker_type == MarkerType::Credits
                                    && m.method == DetectionMethod::Chapter
                                {
                                    m.start = start;
                                    m.end = end;
                                    m.confidence = 0.99;
                                    m.method = DetectionMethod::Fingerprint;
                                }
                            }
                        }
                    }
                    continue;
                }

                let marker = IntroMarker {
                    id: format!("{ep_id}-credits-fp"),
                    start,
                    end,
                    marker_type: MarkerType::Credits,
                    confidence,
                    method: DetectionMethod::Fingerprint,
                };
                res.credits = Some(marker.clone());
                res.markers.push(marker);
            }
        }
    }
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
