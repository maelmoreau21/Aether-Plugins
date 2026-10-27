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

        // Pass 2: Fingerprint-based intro detection with cross-episode pairing
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

        // Pair episodes: (0, 1), (1, 2), ..., and cross-pair (0, 2)
        let mut pair_indices: Vec<(usize, usize)> = Vec::new();
        for i in 0..(fp_episodes.len() - 1) {
            pair_indices.push((i, i + 1));
            if i + 2 < fp_episodes.len() {
                pair_indices.push((i, i + 2));
            }
        }

        // Discovered candidate matches: (ep_id, start, end, score)
        let mut candidates_by_ep: HashMap<String, Vec<(f64, f64, f64)>> = HashMap::new();

        for (idx_a, idx_b) in pair_indices {
            let ep_a = fp_episodes[idx_a];
            let ep_b = fp_episodes[idx_b];
            if let (Some(a), Some(b)) = (ep_a.fingerprint_rate, ep_b.fingerprint_rate)
                && (a - b).abs() > 0.01
            {
                continue;
            }

            // If both already have trusted chapter intro, skip expensive audio matching
            let has_chap_a = results
                .get(&ep_a.id)
                .and_then(|r| r.intro.as_ref())
                .map(|i| i.method == DetectionMethod::Chapter)
                .unwrap_or(false);
            let has_chap_b = results
                .get(&ep_b.id)
                .and_then(|r| r.intro.as_ref())
                .map(|i| i.method == DetectionMethod::Chapter)
                .unwrap_or(false);
            if has_chap_a && has_chap_b {
                continue;
            }

            let fps_a = ep_a.fingerprints.as_ref().unwrap();
            let fps_b = ep_b.fingerprints.as_ref().unwrap();
            let rate = ep_a.fingerprint_rate.or(ep_b.fingerprint_rate);

            let matcher = FingerprintMatcher::new(self.config, rate);
            if let Some(m) = matcher.find_best_match(fps_a, fps_b)
                && m.duration >= self.config.min_intro_duration
                && m.duration <= self.config.max_intro_duration
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

        // Apply consensus: for each episode, pick the most consistent match
        for (ep_id, matches) in candidates_by_ep {
            if matches.is_empty() {
                continue;
            }

            if let Some(res) = results.get_mut(&ep_id) {
                // If chapter intro already exists, don't overwrite unless confidence is low
                if res.intro.is_some() {
                    continue;
                }

                // Average cluster candidate
                let best = matches
                    .iter()
                    .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
                    .cloned();

                if let Some((start, end, score)) = best {
                    let marker = IntroMarker {
                        id: format!("{ep_id}-intro-fp"),
                        start,
                        end,
                        marker_type: MarkerType::Intro,
                        confidence: score,
                        method: DetectionMethod::Fingerprint,
                    };
                    res.intro = Some(marker.clone());
                    res.markers.push(marker);
                }
            }
        }
    }

    /// Run credits detection on episode tails
    fn detect_fingerprint_credits(
        &self,
        episodes: &[EpisodeData],
        results: &mut HashMap<String, EpisodeDetectionResult>,
    ) {
        let detector = CreditsDetector::new(self.config);
        let fp_episodes: Vec<&EpisodeData> = episodes
            .iter()
            .filter(|e| {
                e.fingerprints
                    .as_ref()
                    .map(|f| !f.is_empty())
                    .unwrap_or(false)
            })
            .collect();

        for i in 0..(fp_episodes.len().saturating_sub(1)) {
            let ep_a = fp_episodes[i];
            let ep_b = fp_episodes[i + 1];

            // If both already have chapter credits, skip
            let has_chap_a = results
                .get(&ep_a.id)
                .and_then(|r| r.credits.as_ref())
                .is_some();
            let has_chap_b = results
                .get(&ep_b.id)
                .and_then(|r| r.credits.as_ref())
                .is_some();
            if has_chap_a && has_chap_b {
                continue;
            }

            if let Some((marker_a, marker_b)) = detector.detect_tail_credits(ep_a, ep_b) {
                if let Some(res_a) = results.get_mut(&ep_a.id)
                    && res_a.credits.is_none()
                {
                    res_a.credits = Some(marker_a.clone());
                    res_a.markers.push(marker_a);
                }
                if let Some(res_b) = results.get_mut(&ep_b.id)
                    && res_b.credits.is_none()
                {
                    res_b.credits = Some(marker_b.clone());
                    res_b.markers.push(marker_b);
                }
            }
        }
    }
}
