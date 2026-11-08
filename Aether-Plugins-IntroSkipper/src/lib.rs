//! High-performance Intro & Credits Detection Plugin for Aether.
//! Optimized Rust implementation inspired by https://github.com/intro-skipper/intro-skipper

pub mod adjust;
pub mod chapters;
pub mod consensus;
pub mod credits;
pub mod matcher;
pub mod types;

use aether_plugin_api::{
    ConfigField, ConfigOption, MediaType, PluginCapability, PluginManifest, ProviderConfig,
};
pub use chapters::{ChapterClassifier, MediaKind};
pub use consensus::SeasonAnalyzer;
pub use credits::CreditsDetector;
pub use matcher::{
    DEFAULT_FINGERPRINT_RATE, FingerprintMatcher, LEGACY_FINGERPRINT_RATE, normalize_rate,
};
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
            version: env!("CARGO_PKG_VERSION").into(),
            category: "Playback".into(),
            provider: "Intro Skipper".into(),
            permissions: vec!["playback.markers".into()],
            http_hosts: Vec::new(), // Zero external network access required - pure local analysis
            description: "Détection des introductions, génériques de fin, résumés et aperçus des séries, animés et films : chapitres du fichier, forme des chapitres d'animé et empreintes audio communes à une saison".into(),
            author: "Aether Team".into(),
            license: "AGPL-3.0-only".into(),
            target_abi: "1.0".into(),
            min_server_version: Some("0.1.0".into()),
            max_server_version: Some("1.0.0".into()),
            checksum: None,
            banner: Some("assets/banner.svg".into()),
            capabilities: vec![PluginCapability::Details],
            supported_media_types: vec![
                MediaType::Movie,
                MediaType::TvShow,
                MediaType::Episode,
                MediaType::Anime,
            ],
            default_priority: 50,
            config_schema: config_schema(),
        }
    }
}

/// Request dispatch handling incoming JSON payloads (Wasm or CLI runner)
pub fn handle_intro_skipper_request(
    plugin: &IntroSkipperPlugin,
    req: media_extension_sdk::ExtensionRequest,
) -> media_extension_sdk::ExtensionResponse {
    if req.api_version != 1 {
        return media_extension_sdk::ExtensionResponse {
            api_version: 1,
            result: serde_json::json!({ "error": "unsupported_api_version" }),
        };
    }
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

    if let Err(error) = config.validate() {
        return media_extension_sdk::ExtensionResponse {
            api_version: 1,
            result: serde_json::json!({"error": error}),
        };
    }

    let result_json = match req.operation.as_str() {
        "manifest" => serde_json::to_value(plugin.manifest()).unwrap_or_default(),

        "intro.detect" | "markers.detect" => {
            let mut episodes: Vec<EpisodeData> = req
                .input
                .get("episodes")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            for (episode, raw) in episodes
                .iter_mut()
                .zip(req.input["episodes"].as_array().into_iter().flatten())
            {
                use base64::Engine;
                if let Some(compact) = raw["fingerprintsCompact"].as_str() {
                    episode.fingerprints = base64::engine::general_purpose::STANDARD
                        .decode(compact)
                        .ok()
                        .filter(|bytes| bytes.len() <= 24_000 && bytes.len() % 4 == 0)
                        .map(|bytes| {
                            bytes
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .map(|word| u32::from_le_bytes(*word))
                                .collect()
                        });
                }
                if let Some(compact_tail) = raw["tailFingerprintsCompact"].as_str() {
                    episode.tail_fingerprints = base64::engine::general_purpose::STANDARD
                        .decode(compact_tail)
                        .ok()
                        .filter(|bytes| bytes.len() <= 24_000 && bytes.len() % 4 == 0)
                        .map(|bytes| {
                            bytes
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .map(|word| u32::from_le_bytes(*word))
                                .collect()
                        });
                }
                if let Some(start) = raw["tailStart"]
                    .as_f64()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                {
                    episode.tail_start = Some(start);
                }
            }

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
            if !config.enable_chapter_detection {
                return media_extension_sdk::ExtensionResponse {
                    api_version: 1,
                    result: serde_json::json!([]),
                };
            }
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

            let media = if req.input.get("mediaType").and_then(|v| v.as_str()) == Some("movie") {
                MediaKind::Movie
            } else {
                MediaKind::Episode
            };
            let classifier = ChapterClassifier::new(&config);
            let markers = config.finalize(
                classifier.parse_markers_for(episode_id, &chapters, duration, media),
                duration,
            );
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

fn field(key: &str, label: &str, description: &str, kind: &str, default: &str) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        field_type: kind.into(),
        default_value: Some(default.into()),
        required: false,
        secret: false,
        options: Vec::new(),
    }
}

fn toggle(key: &str, label: &str, description: &str, default: bool) -> ConfigField {
    let (on, off) = (
        ConfigOption {
            label: "Activé".into(),
            value: "true".into(),
        },
        ConfigOption {
            label: "Désactivé".into(),
            value: "false".into(),
        },
    );
    ConfigField {
        options: if default {
            vec![on, off]
        } else {
            vec![off, on]
        },
        ..field(
            key,
            label,
            description,
            "boolean",
            if default { "true" } else { "false" },
        )
    }
}

fn choice(
    key: &str,
    label: &str,
    description: &str,
    default: &str,
    options: &[(&str, &str)],
) -> ConfigField {
    ConfigField {
        options: options
            .iter()
            .map(|(value, label)| ConfigOption {
                label: (*label).into(),
                value: (*value).into(),
            })
            .collect(),
        ..field(key, label, description, "select", default)
    }
}

/// Administration form, in the order it is shown. Every key is read by
/// `IntroSkipperConfig::from_provider_config`; skipping on playback is a viewer
/// preference of the Aether player, not a detection setting.
pub fn config_schema() -> Vec<ConfigField> {
    vec![
        // What is detected
        toggle(
            "detect_intro",
            "Détecter les génériques d'ouverture",
            "Repérer l'intro par chapitres et par empreintes audio communes aux épisodes d'une saison.",
            true,
        ),
        toggle(
            "detect_credits",
            "Détecter les génériques de fin",
            "Repérer le générique de fin pour proposer de le passer ou d'enchaîner l'épisode suivant.",
            true,
        ),
        toggle(
            "detect_recap",
            "Détecter les résumés (« Précédemment »)",
            "Uniquement par chapitres nommés : l'audio d'un résumé change à chaque épisode.",
            true,
        ),
        toggle(
            "detect_preview",
            "Détecter les aperçus de l'épisode suivant",
            "Uniquement par chapitres nommés (« Preview », « Prochain épisode », « Jikai »…).",
            true,
        ),
        // How it is detected
        toggle(
            "enable_chapter_detection",
            "Utiliser les chapitres du fichier",
            "Les chapitres nommés des MKV/MP4 (« Intro », « Opening », « Générique de fin »…) donnent un résultat immédiat et précis.",
            true,
        ),
        toggle(
            "enable_fingerprint_detection",
            "Utiliser l'analyse audio (Chromaprint)",
            "Compare les empreintes audio des épisodes d'une même saison. Nécessite un ffmpeg avec Chromaprint sur le Node.",
            true,
        ),
        toggle(
            "enable_chapter_structure",
            "Deviner l'opening et l'ending des chapitres sans nom",
            "Pour les animés dont les chapitres ne portent pas de nom (« Chapitre 02 ») : les deux chapitres de la durée d'une chanson (75 à 100 s), au début et à la fin, sont proposés comme opening et ending. Confirmés par l'audio, ils sont appliqués.",
            true,
        ),
        field(
            "chapter_snap_window",
            "Caler l'audio sur les chapitres (s)",
            "De 0 à 10 secondes. Une limite trouvée par l'audio à moins de cette distance d'un chapitre se cale sur le chapitre. 0 : jamais.",
            "number",
            "3",
        ),
        // Intro limits
        field(
            "min_intro_duration",
            "Durée minimale de l'intro (s)",
            "De 1 à 600 secondes. Une séquence commune plus courte n'est pas une intro (jingle, logo).",
            "number",
            "15",
        ),
        field(
            "max_intro_duration",
            "Durée maximale de l'intro (s)",
            "Jusqu'à 600 secondes. Les génériques d'animé font souvent 90 s.",
            "number",
            "120",
        ),
        field(
            "search_window_start",
            "Début de la recherche de l'intro (s)",
            "Ignorer l'audio avant cette position (0 à moins de 600 s), par exemple un résumé ou une séquence pré-générique. La fenêtre doit contenir la durée minimale.",
            "number",
            "0",
        ),
        field(
            "search_window_end",
            "Fin de la recherche de l'intro (s)",
            "Plafond 600 secondes. La recherche reste limitée aux empreintes fournies par le Node (environ les 5 premières minutes).",
            "number",
            "600",
        ),
        // Credits limits
        field(
            "min_credits_duration",
            "Durée minimale du générique de fin (s)",
            "De 1 à 300 secondes.",
            "number",
            "10",
        ),
        field(
            "max_credits_duration",
            "Durée maximale du générique de fin (s)",
            "De la durée minimale à 300 secondes. S'applique aux chapitres et à l'analyse audio.",
            "number",
            "300",
        ),
        field(
            "max_movie_credits_duration",
            "Durée maximale du générique de fin d'un film (s)",
            "De la durée minimale à 1200 secondes. Les films n'ont pas d'épisodes à comparer : leur générique vient de leurs chapitres.",
            "number",
            "900",
        ),
        // Acoustic matching
        field(
            "similarity_threshold",
            "Seuil de similarité audio",
            "De 0.50 à 1.00. Plus haut : moins de faux positifs mais plus d'intros manquées (0.70 à 0.85 recommandé).",
            "number",
            "0.75",
        ),
        choice(
            "min_consensus_votes",
            "Accord minimal entre épisodes",
            "Nombre de comparaisons d'épisodes qui doivent retrouver la même séquence. « 2 » écarte les coïncidences mais demande au moins trois épisodes analysés.",
            "1",
            &[
                ("1", "1 — tolérant"),
                ("2", "2 — strict"),
                ("3", "3 — très strict"),
            ],
        ),
        // Boundary adjustments
        field(
            "intro_start_offset",
            "Décalage du début de l'intro (s)",
            "De -30 à 30 secondes, appliqué à chaque intro détectée. Positif : l'intro commence plus tard.",
            "number",
            "0",
        ),
        field(
            "intro_end_offset",
            "Décalage de la fin de l'intro (s)",
            "De -30 à 30 secondes. Négatif : le saut s'arrête plus tôt pour ne pas couper le début de l'épisode.",
            "number",
            "0",
        ),
        field(
            "credits_start_offset",
            "Décalage du début du générique de fin (s)",
            "De -30 à 30 secondes. Positif : garder une scène post-générique ou les dernières répliques.",
            "number",
            "0",
        ),
        // Custom chapter names
        field(
            "intro_keywords",
            "Noms de chapitre supplémentaires — intro",
            "Séparés par des virgules, sans tenir compte de la casse ni des accents. Exemple : Thème, Opening Song.",
            "text",
            "",
        ),
        field(
            "credits_keywords",
            "Noms de chapitre supplémentaires — générique de fin",
            "Séparés par des virgules. Exemple : Staff Roll, Ending Song.",
            "text",
            "",
        ),
        field(
            "recap_keywords",
            "Noms de chapitre supplémentaires — résumé",
            "Séparés par des virgules. Exemple : Dans l'épisode précédent.",
            "text",
            "",
        ),
        field(
            "preview_keywords",
            "Noms de chapitre supplémentaires — aperçu",
            "Séparés par des virgules. Exemple : Bande-annonce, Next Time.",
            "text",
            "",
        ),
    ]
}
