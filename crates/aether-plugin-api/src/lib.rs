//! Common Versioned API and Data Contracts for Aether Metadata Provider Extensions.
//!
//! Re-exports and unifies contracts directly from Aether's official `media-extension-sdk`.

pub use media_extension_sdk::*;

pub mod http;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Plugin Manifest describing the plugin metadata (inspired by Jellyfin plugin system)
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default = "default_category")]
    pub category: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http_hosts: Vec<String>,
    pub description: String,
    pub author: String,
    pub license: String,
    #[serde(default = "default_target_abi")]
    pub target_abi: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_server_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub banner: Option<String>,
    pub capabilities: Vec<PluginCapability>,
    pub supported_media_types: Vec<MediaType>,
    pub default_priority: u32,
    pub config_schema: Vec<ConfigField>,
}

fn default_category() -> String {
    "Metadata".into()
}

fn default_target_abi() -> String {
    "1.0".into()
}

/// Provider Configuration passed at runtime
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl ProviderConfig {
    /// Read the per-installation configuration supplied through the private Wasm input.
    /// Form schema keys use snake_case; the CLI contract also accepts camelCase.
    pub fn from_extension_input(input: &serde_json::Value) -> Self {
        let mut config = input
            .get("config")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(fields) = config.as_object_mut() {
            for (form, wire) in [
                ("api_key", "apiKey"),
                ("user_key", "userKey"),
                ("base_url", "baseUrl"),
                ("timeout_secs", "timeoutSecs"),
            ] {
                if let Some(value) = fields.remove(form) {
                    fields.entry(wire).or_insert(value);
                }
            }
        }
        serde_json::from_value(config).unwrap_or_default()
    }
}

/// Versioned request envelope
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginRequest {
    pub api_version: u32,
    pub operation: String,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub config: ProviderConfig,
}

/// Versioned response envelope
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", bound(deserialize = "T: Deserialize<'de>"))]
pub struct PluginResponse<T> {
    pub api_version: u32,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<PluginError>,
}

impl<T> PluginResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            api_version: API_VERSION,
            success: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(error: PluginError) -> Self {
        Self {
            api_version: API_VERSION,
            success: false,
            data: None,
            error: Some(error),
        }
    }
}

/// The core asynchronous trait implemented by metadata provider extensions
#[async_trait]
pub trait MetadataProviderPlugin: Send + Sync {
    /// Return the static manifest for this extension
    fn manifest(&self) -> PluginManifest;

    /// Search for candidate works on the provider
    async fn search(
        &self,
        query: SearchQuery,
        config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError>;

    /// Fetch complete structured metadata for an identified work
    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError>;

    /// Fetch all available artwork for an identified work
    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError>;
}

/// Dispatcher handling incoming JSON requests from host callers
pub async fn handle_request<P: MetadataProviderPlugin>(plugin: &P, raw_json: &str) -> String {
    let req: PluginRequest = match serde_json::from_str(raw_json) {
        Ok(r) => r,
        Err(e) => {
            let res = PluginResponse::<()>::err(PluginError::new("invalid_json", e.to_string()));
            return serde_json::to_string(&res).unwrap_or_else(|_| "{}".into());
        }
    };

    if req.api_version != API_VERSION {
        let res = PluginResponse::<()>::err(PluginError::new(
            "unsupported_api_version",
            format!(
                "Expected API version {}, got {}",
                API_VERSION, req.api_version
            ),
        ));
        return serde_json::to_string(&res).unwrap_or_else(|_| "{}".into());
    }

    match req.operation.as_str() {
        "manifest" => {
            let manifest = plugin.manifest();
            let res = PluginResponse::ok(manifest);
            serde_json::to_string(&res).unwrap_or_else(|_| "{}".into())
        }
        "search" | "metadata.search" => {
            let search_query: SearchQuery = match serde_json::from_value(req.input) {
                Ok(q) => q,
                Err(e) => {
                    let res = PluginResponse::<()>::err(PluginError::new(
                        "invalid_search_input",
                        e.to_string(),
                    ));
                    return serde_json::to_string(&res).unwrap_or_else(|_| "{}".into());
                }
            };
            match plugin.search(search_query, &req.config).await {
                Ok(items) => serde_json::to_string(&PluginResponse::ok(items))
                    .unwrap_or_else(|_| "{}".into()),
                Err(e) => serde_json::to_string(&PluginResponse::<Vec<SearchResultItem>>::err(e))
                    .unwrap_or_else(|_| "{}".into()),
            }
        }
        "details" | "metadata.details" => {
            let details_req: DetailsRequest = match serde_json::from_value(req.input) {
                Ok(d) => d,
                Err(e) => {
                    let res = PluginResponse::<()>::err(PluginError::new(
                        "invalid_details_input",
                        e.to_string(),
                    ));
                    return serde_json::to_string(&res).unwrap_or_else(|_| "{}".into());
                }
            };
            match plugin.details(details_req, &req.config).await {
                Ok(meta) => {
                    serde_json::to_string(&PluginResponse::ok(meta)).unwrap_or_else(|_| "{}".into())
                }
                Err(e) => serde_json::to_string(&PluginResponse::<StructuredMetadata>::err(e))
                    .unwrap_or_else(|_| "{}".into()),
            }
        }
        "images" | "metadata.images" => {
            let images_req: ImagesRequest = match serde_json::from_value(req.input) {
                Ok(i) => i,
                Err(e) => {
                    let res = PluginResponse::<()>::err(PluginError::new(
                        "invalid_images_input",
                        e.to_string(),
                    ));
                    return serde_json::to_string(&res).unwrap_or_else(|_| "{}".into());
                }
            };
            match plugin.images(images_req, &req.config).await {
                Ok(imgs) => {
                    serde_json::to_string(&PluginResponse::ok(imgs)).unwrap_or_else(|_| "{}".into())
                }
                Err(e) => serde_json::to_string(&PluginResponse::<Vec<MediaImage>>::err(e))
                    .unwrap_or_else(|_| "{}".into()),
            }
        }
        "metadata.propose" => {
            // Backward-compatible operation with Aether Core's original metadata proposal flow
            let title: String = req
                .input
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let query = SearchQuery {
                title: title.clone(),
                year: None,
                media_type: MediaType::Movie,
                language: req.config.language.clone(),
                external_ids: BTreeMap::new(),
            };
            match plugin.search(query, &req.config).await {
                Ok(items) => {
                    if let Some(first) = items.first() {
                        let proposal = MetadataProposal {
                            title: first.title.clone(),
                            summary: first.overview.clone().unwrap_or_default(),
                        };
                        serde_json::to_string(&PluginResponse::ok(proposal))
                            .unwrap_or_else(|_| "{}".into())
                    } else {
                        serde_json::to_string(&PluginResponse::<()>::err(PluginError::not_found(
                            &title,
                        )))
                        .unwrap_or_else(|_| "{}".into())
                    }
                }
                Err(e) => serde_json::to_string(&PluginResponse::<()>::err(e))
                    .unwrap_or_else(|_| "{}".into()),
            }
        }
        unknown => {
            let res = PluginResponse::<()>::err(PluginError::new(
                "unknown_operation",
                format!("Operation '{unknown}' is not supported"),
            ));
            serde_json::to_string(&res).unwrap_or_else(|_| "{}".into())
        }
    }
}

#[cfg(test)]
mod extension_config_tests {
    use super::*;

    #[test]
    fn accepts_form_keys_and_keeps_configuration_per_request() {
        let config = ProviderConfig::from_extension_input(&serde_json::json!({"config": {
            "api_key": "synthetic", "user_key": "user", "language": "fr-FR"
        }}));
        assert_eq!(config.api_key.as_deref(), Some("synthetic"));
        assert_eq!(config.user_key.as_deref(), Some("user"));
        assert_eq!(config.language.as_deref(), Some("fr-FR"));
        assert_eq!(
            ProviderConfig::from_extension_input(&serde_json::json!({})),
            ProviderConfig::default()
        );
    }
}
