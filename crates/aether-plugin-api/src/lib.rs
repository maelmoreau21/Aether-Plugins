//! Common Versioned API and Data Contracts for Aether Metadata Provider Extensions.
//!
//! Re-exports and unifies contracts directly from Aether's official `media-extension-sdk`.
//! Version: 0.1.0

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
    #[serde(default)]
    pub permissions: Vec<String>,
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

impl PluginManifest {
    /// Projects this provider manifest onto the canonical signed `media_extension_sdk::Manifest`.
    pub fn to_sdk_manifest(&self, repository: impl Into<String>) -> Manifest {
        Manifest {
            api_version: self.api_version,
            id: self.id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            description: self.description.clone(),
            author: self.author.clone(),
            license: self.license.clone(),
            repository: repository.into(),
            permissions: self.permissions.clone(),
            http_hosts: self.http_hosts.clone(),
            category: Some(self.category.clone()),
            target_abi: Some(self.target_abi.clone()),
            min_server_version: self.min_server_version.clone(),
            max_server_version: self.max_server_version.clone(),
            checksum: self.checksum.clone(),
            banner: self.banner.clone(),
            changelog: None,
            release_date: None,
            config_schema: if self.config_schema.is_empty() {
                None
            } else {
                Some(self.config_schema.clone())
            },
            storage_migration: None,
        }
    }
}

impl From<&PluginManifest> for Manifest {
    fn from(manifest: &PluginManifest) -> Self {
        manifest.to_sdk_manifest("https://github.com/maelmoreau21/Aether-Plugins")
    }
}

fn default_category() -> String {
    "Metadata".into()
}

fn default_target_abi() -> String {
    "1.0".into()
}

/// Common option consumed by both native HTTP and the Wasm HTTP broker.
pub fn provider_timeout_field() -> ConfigField {
    ConfigField {
        key: "timeout_secs".into(),
        label: "Délai de réponse (secondes)".into(),
        description: "Durée maximale de chaque requête au fournisseur (1 à 120 secondes)".into(),
        field_type: "number".into(),
        required: false,
        secret: false,
        default_value: Some("10".into()),
        options: vec![],
    }
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fallback_languages: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_neutral: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

impl ProviderConfig {
    /// Image-specific overrides are public policy only; credentials stay in config.
    pub fn with_image_input(&self, input: &serde_json::Value) -> Self {
        let mut result = self.clone();
        if let Some(language) = input.get("language").and_then(|v| v.as_str()) {
            result.language = Some(language.to_owned());
        }
        if let Some(languages) = input.get("languages").and_then(|v| v.as_array()) {
            result.languages = languages
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .take(16)
                .collect();
        }
        if let Some(neutral) = input.get("includeNeutral").and_then(|v| v.as_bool()) {
            result.include_neutral = Some(neutral);
        }
        result
    }

    /// Requested language order, preserving regional tags and adding base fallbacks.
    pub fn image_languages(&self) -> Vec<String> {
        let mut result = Vec::new();
        for language in self
            .language
            .iter()
            .chain(self.languages.iter())
            .chain(self.fallback_languages.iter())
            .take(32)
        {
            let tag = language.trim().replace('_', "-").to_ascii_lowercase();
            if tag.len() > 35
                || tag.is_empty()
                || !tag
                    .split('-')
                    .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()))
            {
                continue;
            }
            for value in [tag.as_str(), tag.split('-').next().unwrap_or("")] {
                if !result.iter().any(|v| v == value) {
                    result.push(value.to_owned());
                }
            }
        }
        result
    }

    /// Keep every matching variant, ordered by language, without collapsing image roles.
    /// With no requested language, preserve the legacy unfiltered collection.
    pub fn select_images(&self, mut images: Vec<MediaImage>) -> Vec<MediaImage> {
        let languages = self.image_languages();
        let rank = |image: &MediaImage| {
            let tag = image
                .language
                .as_deref()
                .unwrap_or("")
                .trim()
                .replace('_', "-")
                .to_ascii_lowercase();
            if tag.is_empty() || tag == "00" || tag == "und" || tag == "null" {
                return self
                    .include_neutral
                    .unwrap_or(true)
                    .then_some(languages.len());
            }
            if languages.is_empty() {
                return Some(0);
            }
            languages
                .iter()
                .position(|language| language == &tag)
                .or_else(|| {
                    languages
                        .iter()
                        .position(|language| language == tag.split('-').next().unwrap_or(""))
                })
        };
        images.retain(|image| rank(image).is_some());
        images.sort_by_key(|image| rank(image).unwrap_or(usize::MAX));
        // Leave space for the host response envelope within its 64 KiB limit.
        const MAX_IMAGES: usize = 200;
        const MAX_BYTES: usize = 60 * 1024;
        let mut groups =
            BTreeMap::<(usize, u8, String), std::collections::VecDeque<(MediaImage, usize)>>::new();
        let mut total_bytes = 2usize;
        for image in images {
            let Ok(encoded) = serde_json::to_vec(&image) else {
                continue;
            };
            let bytes = encoded.len() + 1;
            if bytes > MAX_BYTES - 2 {
                continue;
            }
            total_bytes = total_bytes.saturating_add(bytes);
            let role = match image.image_type {
                ImageType::Poster => 0,
                ImageType::Backdrop => 1,
                ImageType::Logo => 2,
                ImageType::ClearArt => 3,
                ImageType::Thumb => 4,
                ImageType::Banner => 5,
                ImageType::DiscArt => 6,
            };
            let tag = image.language.as_deref().unwrap_or("").to_ascii_lowercase();
            groups
                .entry((rank(&image).unwrap_or(usize::MAX), role, tag))
                .or_default()
                .push_back((image, bytes));
        }
        let count: usize = groups.values().map(|group| group.len()).sum();
        // Small responses retain the established language ordering.
        if count <= MAX_IMAGES && total_bytes <= MAX_BYTES {
            return groups
                .into_values()
                .flatten()
                .map(|(image, _)| image)
                .collect();
        }
        // Oversized responses rotate across role/language buckets so a large poster
        // catalogue cannot consume the entire response before logos and fallback art.
        let mut result = Vec::new();
        let mut used_bytes = 2usize;
        loop {
            let mut remaining = false;
            for group in groups.values_mut() {
                if let Some((image, bytes)) = group.pop_front() {
                    remaining = true;
                    if used_bytes + bytes <= MAX_BYTES {
                        used_bytes += bytes;
                        result.push(image);
                        if result.len() == MAX_IMAGES {
                            return result;
                        }
                    }
                }
            }
            if !remaining {
                break;
            }
        }
        result
    }

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
                ("fallback_languages", "fallbackLanguages"),
                ("include_neutral", "includeNeutral"),
            ] {
                if let Some(value) = fields.remove(form) {
                    fields.entry(wire).or_insert(value);
                }
            }
            // Number inputs/defaults may arrive as strings from the admin form.
            // One malformed optional field must not discard valid credentials.
            if let Some(timeout) = fields.get("timeoutSecs") {
                let seconds = timeout
                    .as_u64()
                    .or_else(|| timeout.as_str().and_then(|v| v.parse::<u64>().ok()));
                fields.remove("timeoutSecs");
                if let Some(seconds) = seconds {
                    fields.insert(
                        "timeoutSecs".into(),
                        serde_json::json!(seconds.clamp(1, 120)),
                    );
                }
            }
            // Provider-specific form fields are flat in the signed schema. Preserve
            // them in `extra` instead of silently losing every custom setting.
            let known = [
                "apiKey",
                "userKey",
                "baseUrl",
                "timeoutSecs",
                "language",
                "fallbackLanguages",
                "languages",
                "includeNeutral",
                "extra",
            ];
            let custom: Vec<_> = fields
                .iter()
                .filter(|(key, _)| !known.contains(&key.as_str()))
                .filter_map(|(key, value)| match value {
                    serde_json::Value::String(v) => {
                        Some((key.clone(), serde_json::Value::String(v.clone())))
                    }
                    serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
                        Some((key.clone(), serde_json::Value::String(value.to_string())))
                    }
                    _ => None,
                })
                .collect();
            let extra = fields
                .entry("extra")
                .or_insert_with(|| serde_json::json!({}));
            if let Some(extra) = extra.as_object_mut() {
                for (key, value) in custom {
                    extra.insert(key, value);
                }
            }
        }
        let parsed: Self = serde_json::from_value(config).unwrap_or_default();
        parsed.with_image_input(input)
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

/// Canonical BCP-47-like tag for a provider request: two-letter language, optional
/// upper-case region (`fr`, `fr-FR`, `fr_ca` -> `fr-CA`). `None` for anything else, so a
/// malformed tag from the Core can never reach a provider URL.
pub fn canonical_locale(tag: &str) -> Option<String> {
    let tag = tag.trim().replace('_', "-");
    let mut parts = tag.split('-');
    let language = parts.next()?;
    if language.len() != 2 || !language.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut out = language.to_ascii_lowercase();
    if let Some(region) = parts.next() {
        if region.len() != 2 || !region.bytes().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        out.push('-');
        out.push_str(&region.to_ascii_uppercase());
    }
    parts.next().is_none().then_some(out)
}

/// Build the unique search candidate obtained from an authoritative provider ID.
pub fn identified_search_result(
    metadata: StructuredMetadata,
    media_type: MediaType,
) -> SearchResultItem {
    SearchResultItem {
        id: metadata.provider_id,
        title: metadata.title.unwrap_or_default(),
        artist: None,
        original_title: metadata.original_title,
        year: metadata.year,
        media_type,
        overview: metadata.synopsis,
        poster_url: metadata.posters.first().map(|image| image.url.clone()),
        score: metadata.ratings.first().map(|rating| rating.value),
    }
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
            let image_config = req.config.with_image_input(&req.input);
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
            match plugin.images(images_req, &image_config).await {
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
    fn canonical_locale_keeps_regions_and_rejects_everything_else() {
        for (input, expected) in [
            ("fr", "fr"),
            ("FR-fr", "fr-FR"),
            ("fr_ca", "fr-CA"),
            (" en-GB ", "en-GB"),
        ] {
            assert_eq!(
                canonical_locale(input).as_deref(),
                Some(expected),
                "{input}"
            );
        }
        for bad in [
            "", "f", "fra", "fr-", "fr-FRA", "fr-FR-x", "fr/../x", "fr FR",
        ] {
            assert_eq!(canonical_locale(bad), None, "{bad}");
        }
    }

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

    #[test]
    fn preserves_custom_form_options_and_image_aliases() {
        let config = ProviderConfig::from_extension_input(&serde_json::json!({"config": {
            "api_key": "synthetic", "include_adult": true, "limit": 20,
            "timeout_secs": "15",
            "fallback_languages": ["en"], "include_neutral": false,
            "extra": {"legacy": "kept"}
        }}));
        assert_eq!(config.api_key.as_deref(), Some("synthetic"));
        assert_eq!(config.timeout_secs, Some(15));
        assert_eq!(config.extra["include_adult"], "true");
        assert_eq!(config.extra["limit"], "20");
        assert_eq!(config.extra["legacy"], "kept");
        assert_eq!(config.fallback_languages, ["en"]);
        assert_eq!(config.include_neutral, Some(false));
        let malformed = ProviderConfig::from_extension_input(&serde_json::json!({"config": {
            "api_key": "synthetic", "timeout_secs": "invalid"
        }}));
        assert_eq!(malformed.api_key.as_deref(), Some("synthetic"));
        assert_eq!(malformed.timeout_secs, None);
    }
}
