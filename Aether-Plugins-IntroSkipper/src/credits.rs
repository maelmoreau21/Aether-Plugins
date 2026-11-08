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

    /// Seconds of tail audio compared (the last 5 minutes at most).
    fn tail_seconds(&self) -> f64 {
        self.config.max_credits_duration.max(180.0)
    }

    /// First fingerprint item of the compared tail of an episode.
    fn tail_index(&self, fps: &[u32], rate: f64) -> usize {
        fps.len()
            .saturating_sub((self.tail_seconds() * rate).round() as usize)
    }

    /// Where the compared tail audio of an episode starts in the file, if it has one: credits
    /// found right there may have begun earlier.
    pub fn analysed_tail_start(&self, ep: &EpisodeData) -> Option<f64> {
        let fps = ep.tail_fingerprints.as_deref().filter(|f| !f.is_empty())?;
        let rate = crate::matcher::normalize_rate(ep.fingerprint_rate);
        Some(ep.tail_start? + self.tail_index(fps, rate) as f64 / rate)
    }

    /// Detect credits between two episodes by comparing the tail end audio fingerprints
    pub fn detect_tail_credits(
        &self,
        ep_a: &EpisodeData,
        ep_b: &EpisodeData,
    ) -> Option<(IntroMarker, IntroMarker)> {
        let fps_a = ep_a.tail_fingerprints.as_deref()?;
        let fps_b = ep_b.tail_fingerprints.as_deref()?;
        let tail_start_a = ep_a.tail_start?;
        let tail_start_b = ep_b.tail_start?;

        let rate_a = crate::matcher::normalize_rate(ep_a.fingerprint_rate);
        let rate_b = crate::matcher::normalize_rate(ep_b.fingerprint_rate);
        if (rate_a - rate_b).abs() > 0.01 {
            return None;
        }

        let tail_seconds = self.tail_seconds();
        let start_idx_a = self.tail_index(fps_a, rate_a);
        let start_idx_b = self.tail_index(fps_b, rate_b);

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

        let credits_start_a = tail_start_a + (start_idx_a as f64 / rate_a) + match_result.offset_a;
        let credits_end_a = (credits_start_a + match_result.duration).min(ep_a.duration);

        let credits_start_b = tail_start_b + (start_idx_b as f64 / rate_b) + match_result.offset_b;
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
