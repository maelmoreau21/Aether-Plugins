//! Aether Avatars plugin: avatar packs, random profile picture assignment,
//! and administrative policy enforcement (inspired by jellyfin-avatars).

use aether_plugin_api::{
    ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, ProviderConfig,
    SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Clone, Default)]
pub struct AvatarPlugin;

impl AvatarPlugin {
    pub fn new() -> Self {
        Self
    }

    /// Return list of bundled avatar packs
    pub fn packs(&self) -> Vec<AvatarPackInfo> {
        vec![
            AvatarPackInfo {
                id: "personas".into(),
                name: "Personas".into(),
                description: "Portraits modernes et expressifs avec visages et accessoires variés"
                    .into(),
                count: 10,
                preview: generate_persona_svg(1),
            },
            AvatarPackInfo {
                id: "geometric".into(),
                name: "Géométrique".into(),
                description: "Compositions abstraites et minimalistes aux dégradés soignés".into(),
                count: 10,
                preview: generate_geometric_svg(1),
            },
            AvatarPackInfo {
                id: "retro".into(),
                name: "Rétro Pixel".into(),
                description: "Avatars style pixel-art et nostalgie des jeux vidéo 8-bit".into(),
                count: 10,
                preview: generate_retro_svg(1),
            },
            AvatarPackInfo {
                id: "botts".into(),
                name: "Robots Botts".into(),
                description: "Compagnons robots sympathiques et colorés".into(),
                count: 10,
                preview: generate_bot_svg(1),
            },
        ]
    }

    /// Return all avatars for a specified pack
    pub fn get_pack_avatars(&self, pack_id: &str) -> Vec<AvatarItem> {
        let mut avatars = Vec::new();
        match pack_id {
            "personas" => {
                for i in 1..=10 {
                    avatars.push(AvatarItem {
                        id: format!("personas_{i}"),
                        pack_id: "personas".into(),
                        label: format!("Persona #{i}"),
                        svg: generate_persona_svg(i),
                    });
                }
            }
            "geometric" => {
                for i in 1..=10 {
                    avatars.push(AvatarItem {
                        id: format!("geometric_{i}"),
                        pack_id: "geometric".into(),
                        label: format!("Géométrique #{i}"),
                        svg: generate_geometric_svg(i),
                    });
                }
            }
            "retro" => {
                for i in 1..=10 {
                    avatars.push(AvatarItem {
                        id: format!("retro_{i}"),
                        pack_id: "retro".into(),
                        label: format!("Pixel #{i}"),
                        svg: generate_retro_svg(i),
                    });
                }
            }
            "botts" => {
                for i in 1..=10 {
                    avatars.push(AvatarItem {
                        id: format!("botts_{i}"),
                        pack_id: "botts".into(),
                        label: format!("Bot #{i}"),
                        svg: generate_bot_svg(i),
                    });
                }
            }
            "all" | "" => {
                for pack in &["personas", "geometric", "retro", "botts"] {
                    avatars.extend(self.get_pack_avatars(pack));
                }
            }
            _ => {}
        }
        avatars
    }

    /// Pick a random avatar from the active pack using an optional seed
    pub fn random_avatar(&self, pack_id: Option<&str>, seed: Option<&str>) -> AvatarItem {
        let pack = pack_id.unwrap_or("personas");
        let pool = self.get_pack_avatars(pack);
        if pool.is_empty() {
            let fallback_pool = self.get_pack_avatars("personas");
            return fallback_pool[0].clone();
        }

        let index = if let Some(s) = seed {
            let hash: usize = s
                .bytes()
                .fold(0usize, |acc, b| acc.wrapping_add(b as usize));
            hash % pool.len()
        } else {
            #[cfg(target_arch = "wasm32")]
            {
                0
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                use std::time::SystemTime;
                let nanos = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos() as usize;
                nanos % pool.len()
            }
        };

        pool[index].clone()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPackInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub count: usize,
    pub preview: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarItem {
    pub id: String,
    pub pack_id: String,
    pub label: String,
    pub svg: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarPolicy {
    pub disallow_custom_avatars: bool,
    pub force_random_on_creation: bool,
    pub active_pack: String,
}

#[async_trait]
impl MetadataProviderPlugin for AvatarPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            permissions: vec!["avatars".into()],
            id: "avatars".into(),
            name: "Aether Avatars".into(),
            version: "0.1.0".into(),
            category: "Avatars".into(),
            provider: "aether-avatars".into(),
            http_hosts: vec![],
            description: "Gestionnaire de packs d'avatars et photos de profil (sélection de packs, attribution aléatoire forcée et verrouillage des imports utilisateurs)".into(),
            author: "Aether Team".into(),
            license: "MIT OR Apache-2.0".into(),
            target_abi: "1.0".into(),
            min_server_version: Some("0.1.0".into()),
            max_server_version: Some("1.0.0".into()),
            checksum: Some("b8f3e29104c99738d94e4438df377a06a58bc283995f3b744d08129845ce379a".into()),
            banner: Some(include_str!("../assets/banner.svg").into()),
            capabilities: vec![
                PluginCapability::Details,
                PluginCapability::Images,
            ],
            supported_media_types: vec![MediaType::Movie, MediaType::TvShow],
            default_priority: 50,
            config_schema: vec![
                ConfigField {
                    key: "allow_gravatar".into(),
                    label: "Autoriser l’import depuis Gravatar".into(),
                    description: "Les utilisateurs peuvent importer leur Gravatar depuis leur profil".into(),
                    field_type: "boolean".into(),
                    required: false,
                    secret: false,
                    default_value: Some("false".into()),
                    options: vec![],
                },

                ConfigField {
                    key: "force_random_on_creation".into(),
                    label: "Avatar aléatoire pour les nouveaux utilisateurs".into(),
                    field_type: "boolean".into(),
                    description: "Attribuer automatiquement un avatar aléatoire lors de la création d'un utilisateur sans photo".into(),
                    required: false,
                    secret: false,
                    default_value: Some("true".into()),
                    options: vec!["true".into(), "false".into()],
                },
                ConfigField {
                    key: "disallow_custom_avatars".into(),
                    label: "Verrouiller les imports utilisateurs".into(),
                    field_type: "boolean".into(),
                    description: "Interdire aux utilisateurs d'importer leurs propres images (seuls les avatars du serveur sont autorisés)".into(),
                    required: false,
                    secret: false,
                    default_value: Some("false".into()),
                    options: vec!["false".into(), "true".into()],
                },

            ],
        }
    }

    async fn search(
        &self,
        query: SearchQuery,
        _config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        let packs = self.packs();
        let items = packs
            .into_iter()
            .filter(|p| {
                query.title.is_empty()
                    || p.name.to_lowercase().contains(&query.title.to_lowercase())
                    || p.id.to_lowercase().contains(&query.title.to_lowercase())
            })
            .map(|p| SearchResultItem {
                id: p.id.clone(),
                title: format!("Pack Avatar : {}", p.name),
                original_title: None,
                year: None,
                media_type: MediaType::Movie,
                overview: Some(format!(
                    "{} avatars disponibles. {}",
                    p.count, p.description
                )),
                poster_url: None,
                score: Some(10.0),
            })
            .collect();

        Ok(items)
    }

    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        let pack_id = if request.id.is_empty() {
            config
                .extra
                .get("pack_name")
                .map(|s| s.as_str())
                .unwrap_or("personas")
        } else {
            &request.id
        };

        let avatars = self.get_pack_avatars(pack_id);
        let images = avatars
            .into_iter()
            .map(|a| MediaImage {
                url: format!("data:image/svg+xml;utf8,{}", urlencoding::encode(&a.svg)),
                image_type: ImageType::Thumb,
                width: Some(256),
                height: Some(256),
                language: None,
                rating: None,
                vote_count: None,
                is_primary: false,
            })
            .collect();

        Ok(StructuredMetadata {
            provider: "aether-avatars".into(),
            provider_id: pack_id.into(),
            external_ids: Default::default(),
            title: Some(format!("Pack Avatars : {pack_id}")),
            original_title: None,
            tagline: Some("Pack de photos de profil pour utilisateurs Aether".into()),
            synopsis: Some(
                "Ensemble d'images d'avatars normalisées pour les profils utilisateurs du serveur."
                    .into(),
            ),
            year: None,
            release_date: None,
            runtime_minutes: None,
            genres: vec!["Avatars".into(), "Profils".into()],
            directors: vec![],
            writers: vec![],
            actors: vec![],
            ratings: vec![],
            mpaa: None,
            status: Some("Active".into()),
            posters: vec![],
            backdrops: vec![],
            images,
        })
    }

    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        let details = self
            .details(
                DetailsRequest {
                    id: request.id,
                    media_type: request.media_type,
                    external_ids: request.external_ids,
                },
                config,
            )
            .await?;
        Ok(details.images)
    }
}

// ----------------- SVG Generators for Avatar Packs -----------------

fn generate_persona_svg(idx: usize) -> String {
    let colors = [
        ("#4f46e5", "#818cf8"),
        ("#059669", "#34d399"),
        ("#d97706", "#fbbf24"),
        ("#dc2626", "#f87171"),
        ("#2563eb", "#60a5fa"),
        ("#7c3aed", "#a78bfa"),
        ("#db2777", "#f472b6"),
        ("#0891b2", "#38bdf8"),
        ("#ea580c", "#fb923c"),
        ("#475569", "#94a3b8"),
    ];
    let (bg, accent) = colors[(idx - 1) % colors.len()];
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256"><rect width="256" height="256" rx="128" fill="{bg}"/><circle cx="128" cy="95" r="42" fill="#ffffff"/><path d="M 64,210 C 64,155 92,145 128,145 C 164,145 192,155 192,210 Z" fill="#ffffff" fill-opacity="0.9"/><circle cx="128" cy="95" r="42" fill="{accent}" fill-opacity="0.2"/></svg>"##
    )
}

fn generate_geometric_svg(idx: usize) -> String {
    let colors = [
        ("#1e1b4b", "#6366f1"),
        ("#064e3b", "#10b981"),
        ("#451a03", "#f59e0b"),
        ("#450a0a", "#ef4444"),
        ("#172554", "#3b82f6"),
        ("#2e1065", "#8b5cf6"),
        ("#500724", "#ec4899"),
        ("#083344", "#06b6d4"),
        ("#431407", "#f97316"),
        ("#0f172a", "#64748b"),
    ];
    let (bg, fg) = colors[(idx - 1) % colors.len()];
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256"><rect width="256" height="256" rx="128" fill="{bg}"/><circle cx="128" cy="128" r="64" fill="{fg}" fill-opacity="0.85"/><circle cx="128" cy="128" r="32" fill="#ffffff"/></svg>"##
    )
}

fn generate_retro_svg(idx: usize) -> String {
    let colors = [
        ("#18181b", "#22c55e"),
        ("#18181b", "#06b6d4"),
        ("#18181b", "#f43f5e"),
        ("#18181b", "#eab308"),
        ("#18181b", "#a855f7"),
    ];
    let (bg, fg) = colors[(idx - 1) % colors.len()];
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256"><rect width="256" height="256" rx="128" fill="{bg}"/><circle cx="128" cy="128" r="70" fill="{fg}" fill-opacity="0.15"/><circle cx="100" cy="110" r="12" fill="{fg}"/><circle cx="156" cy="110" r="12" fill="{fg}"/><path d="M 104,155 Q 128,180 152,155" stroke="{fg}" stroke-width="10" stroke-linecap="round" fill="none"/></svg>"##
    )
}

fn generate_bot_svg(idx: usize) -> String {
    let colors = [
        ("#0f172a", "#38bdf8"),
        ("#0f172a", "#4ade80"),
        ("#0f172a", "#f472b6"),
        ("#0f172a", "#fbbf24"),
        ("#0f172a", "#c084fc"),
    ];
    let (bg, fg) = colors[(idx - 1) % colors.len()];
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256" width="256" height="256"><rect width="256" height="256" rx="128" fill="{bg}"/><rect x="80" y="80" width="96" height="80" rx="16" fill="{fg}"/><circle cx="108" cy="115" r="10" fill="#ffffff"/><circle cx="148" cy="115" r="10" fill="#ffffff"/><rect x="110" y="138" width="36" height="6" rx="3" fill="#ffffff"/><line x1="128" y1="80" x2="128" y2="55" stroke="{fg}" stroke-width="6" stroke-linecap="round"/><circle cx="128" cy="50" r="8" fill="{fg}"/></svg>"##
    )
}

mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::with_capacity(s.len() * 2);
        for b in s.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(b as char)
                }
                _ => {
                    use std::fmt::Write;
                    let _ = write!(result, "%{b:02X}");
                }
            }
        }
        result
    }
}

media_extension_sdk::extension!(handle_avatar_extension_request);

pub fn handle_avatar_extension_request(
    req: media_extension_sdk::ExtensionRequest,
) -> media_extension_sdk::ExtensionResponse {
    if req.api_version != 1 {
        return media_extension_sdk::ExtensionResponse {
            api_version: 1,
            result: serde_json::json!({"error": "unsupported_api_version"}),
        };
    }
    let plugin = AvatarPlugin::new();
    let result_json = match req.operation.as_str() {
        "manifest" => serde_json::to_value(plugin.manifest()).unwrap_or_default(),
        "avatars.list" | "avatars" => serde_json::to_value(plugin.packs()).unwrap_or_default(),
        "avatars.generate" => {
            let seed = req.input.get("seed").and_then(|v| v.as_str());
            let pack = req.input.get("pack").and_then(|v| v.as_str());
            let item = plugin.random_avatar(pack, seed);
            serde_json::json!({
                "id": item.id,
                "label": item.label,
                "svg": item.svg,
            })
        }
        "metadata.propose" => {
            serde_json::json!({
                "title": "Avatars",
                "summary": "Pack d'avatars vectoriels pour profils Aether",
            })
        }
        _ => serde_json::json!({ "error": "unsupported_operation" }),
    };

    media_extension_sdk::ExtensionResponse {
        api_version: 1,
        result: result_json,
    }
}
