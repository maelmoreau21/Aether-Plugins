//! High-performance Intro & Credits Detection Plugin for Aether.
//! Optimized Rust implementation inspired by https://github.com/intro-skipper/intro-skipper

pub mod chapters;
pub mod consensus;
pub mod credits;
pub mod matcher;
pub mod types;

use aether_plugin_api::{
    ConfigField, ConfigOption, MediaType, PluginCapability, PluginManifest, ProviderConfig,
};
pub use chapters::ChapterClassifier;
pub use consensus::SeasonAnalyzer;
pub use credits::CreditsDetector;
pub use matcher::{DEFAULT_FINGERPRINT_RATE, FingerprintMatcher};
pub use types::*;

#[derive(Clone, Default)]
pub struct IntroSkipperPlugin;

impl IntroSkipperPlugin {
    pub fn new() -> Self {
        Self
    }

    pub fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: 1,
            id: "intro-skipper".into(),
            name: "Intro Skipper".into(),
            version: "0.1.0".into(),
            category: "Playback".into(),
            provider: "Intro Skipper".into(),
            permissions: vec!["metadata.read".into(), "playback.markers".into()],
            http_hosts: Vec::new(), // Zero external network access required - pure local analysis
            description: "Détection automatique ultra-rapide des introductions et génériques de fin par empreintes audio et chapitres".into(),
            author: "Aether Team".into(),
            license: "AGPL-3.0-only".into(),
            target_abi: "1.0".into(),
            min_server_version: Some("0.1.0".into()),
            max_server_version: Some("1.0.0".into()),
            checksum: None,
            banner: Some("assets/banner.svg".into()),
            capabilities: vec![PluginCapability::Details],
            supported_media_types: vec![MediaType::TvShow, MediaType::Episode, MediaType::Anime],
            default_priority: 50,
            config_schema: vec![
                ConfigField {
                    key: "min_intro_duration".into(),
                    label: "Durée minimale d'intro (s)".into(),
                    description: "Longueur minimale en secondes pour qu'un segment soit identifié comme intro".into(),
                    field_type: "number".into(),
                    default_value: Some("15".into()),
                    required: false,
                    secret: false,
                    options: Vec::new(),
                },
                ConfigField {
                    key: "max_intro_duration".into(),
                    label: "Durée maximale d'intro (s)".into(),
                    description: "Longueur maximale en secondes d'un générique d'ouverture".into(),
                    field_type: "number".into(),
                    default_value: Some("120".into()),
                    required: false,
                    secret: false,
                    options: Vec::new(),
                },
                ConfigField {
                    key: "min_credits_duration".into(),
                    label: "Durée minimale de crédits (s)".into(),
                    description: "Longueur minimale d'un générique de fin".into(),
                    field_type: "number".into(),
                    default_value: Some("10".into()),
                    required: false,
                    secret: false,
                    options: Vec::new(),
                },
                ConfigField {
                    key: "similarity_threshold".into(),
                    label: "Seuil de similarité audio".into(),
                    description: "Seuil de corrélation acoustique (0.70 à 0.90 recommandé)".into(),
                    field_type: "number".into(),
                    default_value: Some("0.75".into()),
                    required: false,
                    secret: false,
                    options: Vec::new(),
                },
                ConfigField {
                    key: "search_window_end".into(),
                    label: "Fenêtre de recherche intro (s)".into(),
                    description: "Rechercher l'intro dans les X premières secondes de l'épisode (600s = 10 min)".into(),
                    field_type: "number".into(),
                    default_value: Some("600".into()),
                    required: false,
                    secret: false,
                    options: Vec::new(),
                },
                ConfigField {
                    key: "auto_skip_intro".into(),
                    label: "Passer l'intro automatiquement".into(),
                    description: "Sauter le générique sans intervention manuelle dès son commencement".into(),
                    field_type: "boolean".into(),
                    default_value: Some("false".into()),
                    required: false,
                    secret: false,
                    options: vec![
                        ConfigOption { label: "Désactivé".into(), value: "false".into() },
                        ConfigOption { label: "Activé".into(), value: "true".into() },
                    ],
                },
                ConfigField {
                    key: "auto_skip_credits".into(),
                    label: "Passer les crédits automatiquement".into(),
                    description: "Passer directement à l'épisode suivant lors du générique de fin".into(),
                    field_type: "boolean".into(),
                    default_value: Some("false".into()),
                    required: false,
                    secret: false,
                    options: vec![
                        ConfigOption { label: "Désactivé".into(), value: "false".into() },
                        ConfigOption { label: "Activé".into(), value: "true".into() },
                    ],
                },
                ConfigField {
                    key: "enable_chapter_detection".into(),
                    label: "Détection par chapitres".into(),
                    description: "Utiliser les chapitres nommés du conteneur MKV/MP4 si disponibles".into(),
                    field_type: "boolean".into(),
                    default_value: Some("true".into()),
                    required: false,
                    secret: false,
                    options: vec![
                        ConfigOption { label: "Activé".into(), value: "true".into() },
                        ConfigOption { label: "Désactivé".into(), value: "false".into() },
                    ],
                },
                ConfigField {
                    key: "enable_fingerprint_detection".into(),
                    label: "Détection acoustique (Chromaprint)".into(),
                    description: "Analyser les empreintes audio pour repérer les séquences récurrentes".into(),
                    field_type: "boolean".into(),
                    default_value: Some("true".into()),
                    required: false,
                    secret: false,
                    options: vec![
                        ConfigOption { label: "Activé".into(), value: "true".into() },
                        ConfigOption { label: "Désactivé".into(), value: "false".into() },
                    ],
                },
            ],
        }
    }
}

/// Request dispatch handling incoming JSON payloads (Wasm or CLI runner)
pub fn handle_intro_skipper_request(
    plugin: &IntroSkipperPlugin,
    req: media_extension_sdk::ExtensionRequest,
) -> media_extension_sdk::ExtensionResponse {
    let config = if let Some(cfg_val) = req.input.get("config") {
        let mut provider_config: ProviderConfig =
            serde_json::from_value(cfg_val.clone()).unwrap_or_default();
        if let Some(settings) = cfg_val.as_object() {
            for (key, value) in settings {
                if let Some(value) = value.as_str() {
                    provider_config.extra.insert(key.clone(), value.to_owned());
                } else if value.is_boolean() || value.is_number() {
                    provider_config.extra.insert(key.clone(), value.to_string());
                }
            }
        }
        IntroSkipperConfig::from_provider_config(&provider_config)
    } else {
        IntroSkipperConfig::default()
    };

    let result_json = match req.operation.as_str() {
        "manifest" => serde_json::to_value(plugin.manifest()).unwrap_or_default(),

        "intro.detect" | "markers.detect" => {
            let episodes: Vec<EpisodeData> = req
                .input
                .get("episodes")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            let analyzer = SeasonAnalyzer::new(&config);
            let results = analyzer.analyze_season(&episodes);
            serde_json::to_value(results).unwrap_or_default()
        }

        "intro.match_fingerprints" => {
            let fps_a: Vec<u32> = req
                .input
                .get("a")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let fps_b: Vec<u32> = req
                .input
                .get("b")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let rate: Option<f64> = req.input.get("rate").and_then(|v| v.as_f64());

            let matcher = FingerprintMatcher::new(&config, rate);
            let match_res = matcher.find_best_match(&fps_a, &fps_b);
            serde_json::to_value(match_res).unwrap_or_default()
        }

        "intro.parse_chapters" => {
            let episode_id = req
                .input
                .get("episodeId")
                .and_then(|v| v.as_str())
                .unwrap_or("ep");
            let duration = req
                .input
                .get("duration")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let chapters: Vec<ChapterItem> = req
                .input
                .get("chapters")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();

            let classifier = ChapterClassifier::new(&config);
            let markers = classifier.parse_markers(episode_id, &chapters, duration);
            serde_json::to_value(markers).unwrap_or_default()
        }

        "metadata.propose" => serde_json::json!({
            "title": "Intro Skipper",
            "summary": "Détection automatique des introductions et génériques de séries",
        }),

        _ => serde_json::json!({ "error": "unsupported_operation" }),
    };

    media_extension_sdk::ExtensionResponse {
        api_version: 1,
        result: result_json,
    }
}

// Wasm entrypoints
media_extension_sdk::extension!(handle_raw_extension_request);

pub fn handle_raw_extension_request(
    req: media_extension_sdk::ExtensionRequest,
) -> media_extension_sdk::ExtensionResponse {
    let plugin = IntroSkipperPlugin::new();
    handle_intro_skipper_request(&plugin, req)
}
