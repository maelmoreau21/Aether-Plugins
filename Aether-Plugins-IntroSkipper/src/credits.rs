use crate::matcher::FingerprintMatcher;
use crate::types::{DetectionMethod, EpisodeData, IntroMarker, IntroSkipperConfig, MarkerType};

/// Credit and outro detection engine
pub struct CreditsDetector<'a> {
    config: &'a IntroSkipperConfig,
}

impl<'a> CreditsDetector<'a> {
    pub fn new(config: &'a IntroSkipperConfig) -> Self {
        Self { config }
    }

    /// Detect credits between two episodes by comparing the tail end audio fingerprints
    pub fn detect_tail_credits(
        &self,
        ep_a: &EpisodeData,
        ep_b: &EpisodeData,
    ) -> Option<(IntroMarker, IntroMarker)> {
        let fps_a = ep_a.fingerprints.as_deref()?;
        let fps_b = ep_b.fingerprints.as_deref()?;

        let rate_a = ep_a
            .fingerprint_rate
            .unwrap_or(crate::matcher::DEFAULT_FINGERPRINT_RATE);
        let rate_b = ep_b
            .fingerprint_rate
            .unwrap_or(crate::matcher::DEFAULT_FINGERPRINT_RATE);

        // Analyze last 5 minutes (300 seconds) of each episode
        let tail_seconds = self.config.max_credits_duration.max(180.0);
        let tail_frames_a = (tail_seconds * rate_a).round() as usize;
        let tail_frames_b = (tail_seconds * rate_b).round() as usize;

        let start_idx_a = fps_a.len().saturating_sub(tail_frames_a);
        let start_idx_b = fps_b.len().saturating_sub(tail_frames_b);

        let slice_a = &fps_a[start_idx_a..];
        let slice_b = &fps_b[start_idx_b..];

        if slice_a.is_empty() || slice_b.is_empty() {
            return None;
        }

        // Tail config with modified window matching
        let mut tail_config = self.config.clone();
        tail_config.min_intro_duration = self.config.min_credits_duration;
        tail_config.max_intro_duration = self.config.max_credits_duration;
        tail_config.search_window_start = 0.0;
        tail_config.search_window_end = tail_seconds;

        let matcher = FingerprintMatcher::new(&tail_config, Some(rate_a));
        let match_result = matcher.find_best_match(slice_a, slice_b)?;

        let credits_start_a = (start_idx_a as f64 / rate_a) + match_result.offset_a;
        let credits_end_a = (credits_start_a + match_result.duration).min(ep_a.duration);

        let credits_start_b = (start_idx_b as f64 / rate_b) + match_result.offset_b;
        let credits_end_b = (credits_start_b + match_result.duration).min(ep_b.duration);

        let marker_a = IntroMarker {
            id: format!("{}-credits-fp", ep_a.id),
            start: credits_start_a,
            end: credits_end_a,
            marker_type: MarkerType::Credits,
            confidence: match_result.score,
            method: DetectionMethod::Fingerprint,
        };

        let marker_b = IntroMarker {
            id: format!("{}-credits-fp", ep_b.id),
            start: credits_start_b,
            end: credits_end_b,
            marker_type: MarkerType::Credits,
            confidence: match_result.score,
            method: DetectionMethod::Fingerprint,
        };

        Some((marker_a, marker_b))
    }
}
