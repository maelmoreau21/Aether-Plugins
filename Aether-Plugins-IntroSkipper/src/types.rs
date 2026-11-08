use serde::{Deserialize, Serialize};

/// Type of media segment marker
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerType {
    Intro,
    Credits,
    Recap,
    Preview,
}

impl std::fmt::Display for MarkerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Intro => write!(f, "intro"),
            Self::Credits => write!(f, "credits"),
            Self::Recap => write!(f, "recap"),
            Self::Preview => write!(f, "preview"),
        }
    }
}

/// How the segment marker was discovered
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    Fingerprint,
    Chapter,
    Silence,
    Manual,
}

/// A detected media timeline marker (e.g. Intro or Credits)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntroMarker {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub marker_type: MarkerType,
    pub confidence: f64,
    pub method: DetectionMethod,
}

impl IntroMarker {
    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}

/// Chapter information from container metadata
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterItem {
    pub start: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    pub title: String,
}

/// Episode representation with fingerprints and/or chapters
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeData {
    pub id: String,
    #[serde(default)]
    pub series_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub season_number: Option<u32>,
    #[serde(default)]
    pub episode_number: Option<u32>,
    pub duration: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprints: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_fingerprints: Option<Vec<u32>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_start: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chapters: Option<Vec<ChapterItem>>,
}

/// Runtime configuration options for Intro Skipper
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntroSkipperConfig {
    pub min_intro_duration: f64,
    pub max_intro_duration: f64,
    pub min_credits_duration: f64,
    pub max_credits_duration: f64,
    /// Longest credits chapter accepted for a film (film credits often run 8 to 15 minutes).
    pub max_movie_credits_duration: f64,
    pub similarity_threshold: f64,
    pub search_window_start: f64,
    pub search_window_end: f64,
    pub enable_chapter_detection: bool,
    pub enable_fingerprint_detection: bool,
    /// Guess the opening and ending of an anime episode from the length and position of its
    /// unnamed chapters.
    pub enable_chapter_structure: bool,
    /// Acoustic boundaries move to a chapter mark this close (seconds, 0 = never).
    pub chapter_snap_window: f64,
    /// Which segment kinds are reported at all.
    pub detect_intro: bool,
    pub detect_credits: bool,
    pub detect_recap: bool,
    pub detect_preview: bool,
    /// Agreeing episode comparisons an acoustic segment needs before it is reported. Capped at
    /// the comparisons a batch allows (two per episode in the Core's three-episode batches);
    /// values 3 and 4 are still accepted from older settings.
    pub min_consensus_votes: u32,
    /// Seconds added to the detected boundaries (negative values move them earlier).
    pub intro_start_offset: f64,
    pub intro_end_offset: f64,
    pub credits_start_offset: f64,
    /// Extra chapter names, matched like the built-in ones (case and accents ignored).
    pub intro_keywords: Vec<String>,
    pub credits_keywords: Vec<String>,
    pub recap_keywords: Vec<String>,
    pub preview_keywords: Vec<String>,
}

impl Default for IntroSkipperConfig {
    fn default() -> Self {
        Self {
            min_intro_duration: 15.0,
            max_intro_duration: 120.0,
            min_credits_duration: 10.0,
            max_credits_duration: 300.0,
            max_movie_credits_duration: 900.0,
            similarity_threshold: 0.75,
            search_window_start: 0.0,
            search_window_end: 600.0, // First 10 minutes
            enable_chapter_detection: true,
            enable_fingerprint_detection: true,
            enable_chapter_structure: true,
            chapter_snap_window: 3.0,
            detect_intro: true,
            detect_credits: true,
            detect_recap: true,
            detect_preview: true,
            min_consensus_votes: 1,
            intro_start_offset: 0.0,
            intro_end_offset: 0.0,
            credits_start_offset: 0.0,
            intro_keywords: Vec::new(),
            credits_keywords: Vec::new(),
            recap_keywords: Vec::new(),
            preview_keywords: Vec::new(),
        }
    }
}

/// Largest boundary shift accepted, in seconds, in either direction.
pub const MAX_BOUNDARY_OFFSET: f64 = 30.0;
/// Longest credits segment the Aether host stores (seconds).
pub const MAX_CREDITS_SEGMENT: f64 = 1200.0;
/// Longest intro, recap or preview segment the Aether host stores (seconds).
pub const MAX_OTHER_SEGMENT: f64 = 600.0;
const MAX_KEYWORDS: usize = 32;
const MAX_KEYWORD_LEN: usize = 64;

impl IntroSkipperConfig {
    /// Settings are bounded independently of the supplied fingerprint length.
    /// Invalid settings fail before fingerprint comparisons.
    pub fn validate(&self) -> Result<(), &'static str> {
        let offsets = [
            self.intro_start_offset,
            self.intro_end_offset,
            self.credits_start_offset,
        ];
        if ![
            self.min_intro_duration,
            self.max_intro_duration,
            self.min_credits_duration,
            self.max_credits_duration,
            self.max_movie_credits_duration,
            self.chapter_snap_window,
            self.search_window_start,
            self.search_window_end,
            self.similarity_threshold,
        ]
        .iter()
        .chain(offsets.iter())
        .all(|value| value.is_finite())
            || !(1.0..=600.0).contains(&self.min_intro_duration)
            || !(self.min_intro_duration..=600.0).contains(&self.max_intro_duration)
            || !(1.0..=300.0).contains(&self.min_credits_duration)
            || !(self.min_credits_duration..=300.0).contains(&self.max_credits_duration)
            || !(self.min_credits_duration..=1200.0).contains(&self.max_movie_credits_duration)
            || !(0.0..=10.0).contains(&self.chapter_snap_window)
            || !(0.0..600.0).contains(&self.search_window_start)
            || !(self.search_window_start..=600.0).contains(&self.search_window_end)
            || self.search_window_end - self.search_window_start < self.min_intro_duration
            || !(0.5..=1.0).contains(&self.similarity_threshold)
            || !(1..=4).contains(&self.min_consensus_votes)
            || offsets
                .iter()
                .any(|offset| offset.abs() > MAX_BOUNDARY_OFFSET)
        {
            return Err("invalid_detection_config");
        }
        Ok(())
    }

    /// Whether a marker of this kind may be reported.
    pub fn reports(&self, kind: MarkerType) -> bool {
        match kind {
            MarkerType::Intro => self.detect_intro,
            MarkerType::Credits => self.detect_credits,
            MarkerType::Recap => self.detect_recap,
            MarkerType::Preview => self.detect_preview,
        }
    }

    /// Final pass shared by every operation: drops disabled kinds and applies the
    /// configured boundary offsets, clamped to the episode. A segment shorter than one
    /// second after adjustment is dropped rather than reported. A segment longer than the host
    /// accepts ([`MAX_CREDITS_SEGMENT`], [`MAX_OTHER_SEGMENT`]) is shortened, since one marker
    /// over the limit makes the host reject the whole answer: credits keep their end, other
    /// segments their start.
    pub fn finalize(&self, markers: Vec<IntroMarker>, duration: f64) -> Vec<IntroMarker> {
        let limit = if duration.is_finite() && duration > 0.0 {
            duration
        } else {
            f64::MAX
        };
        markers
            .into_iter()
            .filter(|marker| self.reports(marker.marker_type))
            .filter_map(|mut marker| {
                match marker.marker_type {
                    MarkerType::Intro => {
                        marker.start += self.intro_start_offset;
                        marker.end += self.intro_end_offset;
                    }
                    MarkerType::Credits => marker.start += self.credits_start_offset,
                    MarkerType::Recap | MarkerType::Preview => {}
                }
                marker.start = marker.start.clamp(0.0, limit);
                marker.end = marker.end.clamp(0.0, limit);
                if marker.marker_type == MarkerType::Credits {
                    marker.start = marker.start.max(marker.end - MAX_CREDITS_SEGMENT);
                } else {
                    marker.end = marker.end.min(marker.start + MAX_OTHER_SEGMENT);
                }
                (marker.end - marker.start >= 1.0).then_some(marker)
            })
            .collect()
    }

    pub fn from_provider_config(cfg: &aether_plugin_api::ProviderConfig) -> Self {
        let mut base = Self::default();
        for (key, value) in [
            ("min_intro_duration", &mut base.min_intro_duration),
            ("max_intro_duration", &mut base.max_intro_duration),
            ("min_credits_duration", &mut base.min_credits_duration),
            ("max_credits_duration", &mut base.max_credits_duration),
            (
                "max_movie_credits_duration",
                &mut base.max_movie_credits_duration,
            ),
            ("chapter_snap_window", &mut base.chapter_snap_window),
            ("similarity_threshold", &mut base.similarity_threshold),
            ("search_window_start", &mut base.search_window_start),
            ("search_window_end", &mut base.search_window_end),
            ("intro_start_offset", &mut base.intro_start_offset),
            ("intro_end_offset", &mut base.intro_end_offset),
            ("credits_start_offset", &mut base.credits_start_offset),
        ] {
            // An empty form field keeps the default; anything else must parse.
            if let Some(raw) = cfg.extra.get(key).filter(|raw| !raw.trim().is_empty()) {
                *value = raw.trim().parse().unwrap_or(f64::NAN);
            }
        }
        for (key, value) in [
            (
                "enable_chapter_detection",
                &mut base.enable_chapter_detection,
            ),
            (
                "enable_fingerprint_detection",
                &mut base.enable_fingerprint_detection,
            ),
            (
                "enable_chapter_structure",
                &mut base.enable_chapter_structure,
            ),
            ("detect_intro", &mut base.detect_intro),
            ("detect_credits", &mut base.detect_credits),
            ("detect_recap", &mut base.detect_recap),
            ("detect_preview", &mut base.detect_preview),
        ] {
            if let Some(parsed) = cfg.extra.get(key).and_then(|s| s.trim().parse().ok()) {
                *value = parsed;
            }
        }
        if let Some(raw) = cfg
            .extra
            .get("min_consensus_votes")
            .filter(|raw| !raw.trim().is_empty())
        {
            // "2" and "2.0" are both accepted; anything else fails validation.
            base.min_consensus_votes = raw
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| n.fract() == 0.0 && (1.0..=4.0).contains(n))
                .map_or(0, |n| n as u32);
        }
        for (key, value) in [
            ("intro_keywords", &mut base.intro_keywords),
            ("credits_keywords", &mut base.credits_keywords),
            ("recap_keywords", &mut base.recap_keywords),
            ("preview_keywords", &mut base.preview_keywords),
        ] {
            if let Some(raw) = cfg.extra.get(key) {
                *value = parse_keywords(raw);
            }
        }
        base
    }
}

/// Comma, semicolon or newline separated chapter names, normalised like chapter titles.
pub fn parse_keywords(raw: &str) -> Vec<String> {
    let mut keywords: Vec<String> = Vec::new();
    for item in raw.split([',', ';', '\n']) {
        let clean = crate::chapters::clean_text(item);
        if !clean.is_empty() && clean.len() <= MAX_KEYWORD_LEN && !keywords.contains(&clean) {
            keywords.push(clean);
        }
        if keywords.len() == MAX_KEYWORDS {
            break;
        }
    }
    keywords
}

/// Analysis and detection result for one episode
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeDetectionResult {
    pub episode_id: String,
    pub markers: Vec<IntroMarker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intro: Option<IntroMarker>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credits: Option<IntroMarker>,
}

/// Match interval found between two fingerprint streams
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FingerprintMatch {
    pub offset_a: f64,
    pub offset_b: f64,
    pub duration: f64,
    pub score: f64,
}
