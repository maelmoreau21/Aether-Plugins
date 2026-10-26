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
    pub similarity_threshold: f64,
    pub search_window_start: f64,
    pub search_window_end: f64,
    pub auto_skip_intro: bool,
    pub auto_skip_credits: bool,
    pub enable_chapter_detection: bool,
    pub enable_fingerprint_detection: bool,
}

impl Default for IntroSkipperConfig {
    fn default() -> Self {
        Self {
            min_intro_duration: 15.0,
            max_intro_duration: 120.0,
            min_credits_duration: 10.0,
            max_credits_duration: 300.0,
            similarity_threshold: 0.75,
            search_window_start: 0.0,
            search_window_end: 600.0, // First 10 minutes
            auto_skip_intro: false,
            auto_skip_credits: false,
            enable_chapter_detection: true,
            enable_fingerprint_detection: true,
        }
    }
}

impl IntroSkipperConfig {
    pub fn from_provider_config(cfg: &aether_plugin_api::ProviderConfig) -> Self {
        let mut base = Self::default();
        if let Some(val) = cfg
            .extra
            .get("min_intro_duration")
            .and_then(|s| s.parse().ok())
        {
            base.min_intro_duration = val;
        }
        if let Some(val) = cfg
            .extra
            .get("max_intro_duration")
            .and_then(|s| s.parse().ok())
        {
            base.max_intro_duration = val;
        }
        if let Some(val) = cfg
            .extra
            .get("min_credits_duration")
            .and_then(|s| s.parse().ok())
        {
            base.min_credits_duration = val;
        }
        if let Some(val) = cfg
            .extra
            .get("similarity_threshold")
            .and_then(|s| s.parse().ok())
        {
            base.similarity_threshold = val;
        }
        if let Some(val) = cfg
            .extra
            .get("search_window_end")
            .and_then(|s| s.parse().ok())
        {
            base.search_window_end = val;
        }
        if let Some(val) = cfg
            .extra
            .get("auto_skip_intro")
            .and_then(|s| s.parse().ok())
        {
            base.auto_skip_intro = val;
        }
        if let Some(val) = cfg
            .extra
            .get("auto_skip_credits")
            .and_then(|s| s.parse().ok())
        {
            base.auto_skip_credits = val;
        }
        if let Some(val) = cfg
            .extra
            .get("enable_chapter_detection")
            .and_then(|s| s.parse().ok())
        {
            base.enable_chapter_detection = val;
        }
        if let Some(val) = cfg
            .extra
            .get("enable_fingerprint_detection")
            .and_then(|s| s.parse().ok())
        {
            base.enable_fingerprint_detection = val;
        }
        base
    }
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
