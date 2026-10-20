//! Common Versioned API and Data Contracts for Aether Metadata Provider Extensions.
//!
//! # Architecture Rules:
//! - Core -> Extensions: Aether Core decides when to search or refresh metadata.
//! - Extensions query external providers (TMDb, IMDb, fanart.tv, etc.) and return structured data.
//! - Aether remains strictly responsible for identification, provider priority, merging, validation, caching and database storage.
//! - Extensions NEVER access the database directly and NEVER modify media files.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const API_VERSION: u32 = 1;

/// Supported media types
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Movie,
    TvShow,
    Episode,
    Anime,
}

/// Capabilities exposed by a plugin
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    Search,
    Details,
    Images,
}

/// Image category
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageType {
    Poster,
    Backdrop,
    Banner,
    Logo,
    ClearArt,
    Thumb,
    DiscArt,
}

/// A media image (poster, backdrop, logo, etc.)
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaImage {
    pub url: String,
    pub image_type: ImageType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rating: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vote_count: Option<u64>,
    #[serde(default)]
    pub is_primary: bool,
}

/// An actor/cast member credit
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorCredit {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumb_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
}

/// A rating score from a specific provider
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRating {
    pub provider: String,
    pub value: f64,
    pub max: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub votes: Option<u64>,
}

/// Full structured metadata returned by a provider
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredMetadata {
    pub provider: String,
    pub provider_id: String,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synopsis: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_minutes: Option<u32>,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub directors: Vec<String>,
    #[serde(default)]
    pub writers: Vec<String>,
    #[serde(default)]
    pub actors: Vec<ActorCredit>,
    #[serde(default)]
    pub ratings: Vec<ProviderRating>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mpaa: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default)]
    pub posters: Vec<MediaImage>,
    #[serde(default)]
    pub backdrops: Vec<MediaImage>,
    #[serde(default)]
    pub images: Vec<MediaImage>,
}

/// Search query payload sent to the extension
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    #[serde(default = "default_media_type")]
    pub media_type: MediaType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
}

fn default_media_type() -> MediaType {
    MediaType::Movie
}

/// Individual item in search results
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultItem {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    pub media_type: MediaType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

/// Configuration field descriptor
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub description: String,
    pub required: bool,
    pub secret: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
}

/// Plugin Manifest describing the plugin metadata
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub provider: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub capabilities: Vec<PluginCapability>,
    pub supported_media_types: Vec<MediaType>,
    pub default_priority: u32,
    pub config_schema: Vec<ConfigField>,
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

/// Error returned by a plugin
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]: {}", self.code, self.message)
    }
}

impl std::error::Error for PluginError {}

impl PluginError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retry_after_secs: None,
        }
    }

    pub fn rate_limited(retry_after_secs: u64) -> Self {
        Self {
            code: "rate_limited".into(),
            message: format!("Provider rate limit exceeded. Retry after {retry_after_secs}s"),
            retry_after_secs: Some(retry_after_secs),
        }
    }

    pub fn missing_api_key(provider: &str) -> Self {
        Self {
            code: "missing_api_key".into(),
            message: format!("API key is required for provider '{provider}'"),
            retry_after_secs: None,
        }
    }

    pub fn not_found(id: &str) -> Self {
        Self {
            code: "not_found".into(),
            message: format!("Media '{id}' not found on provider"),
            retry_after_secs: None,
        }
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

/// Details request payload
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetailsRequest {
    pub id: String,
    #[serde(default = "default_media_type")]
    pub media_type: MediaType,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
}

/// Images request payload
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagesRequest {
    pub id: String,
    #[serde(default = "default_media_type")]
    pub media_type: MediaType,
    #[serde(default)]
    pub external_ids: BTreeMap<String, String>,
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

/// Dispatcher handling incoming JSON requests from stdin or API callers
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
        "search" => {
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
        "details" => {
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
        "images" => {
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
        unknown => {
            let res = PluginResponse::<()>::err(PluginError::new(
                "unknown_operation",
                format!("Operation '{unknown}' is not supported"),
            ));
            serde_json::to_string(&res).unwrap_or_else(|_| "{}".into())
        }
    }
}
