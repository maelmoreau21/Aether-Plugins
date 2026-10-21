//! IMDb (Internet Movie Database) metadata provider extension for Aether.

use aether_plugin_api::{
    ActorCredit, ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProposal, MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest,
    ProviderConfig, ProviderRating, SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
#[cfg(not(target_arch = "wasm32"))]
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

const DEFAULT_SUGGESTION_URL: &str = "https://v3.sg.media-imdb.com/suggestion";
const DEFAULT_TITLE_URL: &str = "https://www.imdb.com/title";

#[derive(Clone, Default)]
pub struct ImdbPlugin;

impl ImdbPlugin {
    pub fn new() -> Self {
        Self
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn build_client(&self, config: &ProviderConfig) -> Result<reqwest::Client, PluginError> {
        let timeout = Duration::from_secs(config.timeout_secs.unwrap_or(10));
        let mut headers = HeaderMap::new();
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36 Aether/0.1.0"),
        );
        headers.insert(
            "Accept",
            HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,application/json;q=0.8,*/*;q=0.7"),
        );
        headers.insert(
            "Accept-Language",
            HeaderValue::from_static("fr-FR,fr;q=0.9,en-US;q=0.8,en;q=0.7"),
        );

        reqwest::Client::builder()
            .timeout(timeout)
            .default_headers(headers)
            .build()
            .map_err(|e| PluginError::new("client_error", e.to_string()))
    }

    async fn fetch_text(&self, url: &str, config: &ProviderConfig) -> Result<String, PluginError> {
        #[cfg(target_arch = "wasm32")]
        {
            let mut headers = BTreeMap::new();
            headers.insert(
                "User-Agent".into(),
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36 Aether/0.1.0".into(),
            );
            headers.insert(
                "Accept".into(),
                "text/html,application/xhtml+xml,application/xml;q=0.9,application/json;q=0.8,*/*;q=0.7".into(),
            );
            headers.insert(
                "Accept-Language".into(),
                "fr-FR,fr;q=0.9,en-US;q=0.8,en;q=0.7".into(),
            );

            let host_req = media_extension_sdk::HostHttpRequest {
                url: url.to_string(),
                method: "GET".into(),
                headers,
                body: None,
                timeout_secs: config.timeout_secs.map(|s| s as u32),
            };

            let resp = media_extension_sdk::host_http_fetch(&host_req)
                .map_err(|e| PluginError::new(e.code, e.message))?;

            if resp.status == 404 {
                return Err(PluginError::not_found(url));
            }
            if resp.status == 429 {
                return Err(PluginError::rate_limited(10));
            }
            if resp.status != 200 {
                return Err(PluginError::new(
                    "provider_error",
                    format!("IMDb page returned HTTP status {}", resp.status),
                ));
            }

            String::from_utf8(resp.body).map_err(|e| PluginError::new("read_error", e.to_string()))
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let client = self.build_client(config)?;
            let res = client
                .get(url)
                .send()
                .await
                .map_err(|e| PluginError::new("network_error", e.to_string()))?;

            if res.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(PluginError::not_found(url));
            }
            if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(PluginError::rate_limited(10));
            }
            if !res.status().is_success() {
                return Err(PluginError::new(
                    "provider_error",
                    format!("IMDb page returned status {}", res.status()),
                ));
            }

            res.text()
                .await
                .map_err(|e| PluginError::new("read_error", e.to_string()))
        }
    }

    fn normalize_query(&self, raw: &str) -> String {
        raw.chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
            .collect::<String>()
            .trim()
            .to_lowercase()
            .replace(' ', "_")
    }

    fn parse_iso_duration(&self, duration: &str) -> Option<u32> {
        // e.g. "PT2H28M" or "PT148M" or "PT45M"
        if !duration.starts_with("PT") {
            return None;
        }
        let rest = &duration[2..];
        let mut total_minutes = 0u32;
        let mut current_num = String::new();

        for ch in rest.chars() {
            if ch.is_ascii_digit() {
                current_num.push(ch);
            } else if ch == 'H' {
                if let Ok(hours) = current_num.parse::<u32>() {
                    total_minutes += hours * 60;
                }
                current_num.clear();
            } else if ch == 'M' {
                if let Ok(mins) = current_num.parse::<u32>() {
                    total_minutes += mins;
                }
                current_num.clear();
            }
        }
        if total_minutes > 0 {
            Some(total_minutes)
        } else {
            None
        }
    }
}

#[async_trait]
impl MetadataProviderPlugin for ImdbPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            id: "imdb".into(),
            name: "IMDb (Internet Movie Database)".into(),
            version: "0.1.0".into(),
            category: "Metadata".into(),
            provider: "imdb".into(),
            http_hosts: vec!["v3.sg.media-imdb.com".into(), "imdb.com".into(), "www.imdb.com".into()],
            description: "Fournisseur de métadonnées IMDb (notes mondiales de référence, votes, casting et recherche instantanée)".into(),
            author: "Aether Team".into(),
            license: "MIT OR Apache-2.0".into(),
            target_abi: "1.0".into(),
            min_server_version: Some("0.1.0".into()),
            max_server_version: Some("1.0.0".into()),
            checksum: Some("e2b9d997222b9d0f615d66efa018b93fc90621c760dbe4655fecb15354796f2c".into()),
            banner: Some(include_str!("../assets/banner.svg").into()),
            capabilities: vec![
                PluginCapability::Search,
                PluginCapability::Details,
                PluginCapability::Images,
            ],
            supported_media_types: vec![
                MediaType::Movie,
                MediaType::TvShow,
                MediaType::Episode,
                MediaType::Anime,
            ],
            default_priority: 80,
            config_schema: vec![
                ConfigField {
                    key: "api_key".into(),
                    label: "Clé API (optionnelle)".into(),
                    field_type: "password".into(),
                    description: "Clé API optionnelle si vous utilisez un proxy IMDb / RapidAPI".into(),
                    required: false,
                    secret: true,
                    default_value: None,
                    options: vec![],
                },
                ConfigField {
                    key: "language".into(),
                    label: "Langue préférée".into(),
                    field_type: "select".into(),
                    description: "Code langue pour l'en-tête Accept-Language (ex. fr-FR, en-US)".into(),
                    required: false,
                    secret: false,
                    default_value: Some("fr-FR".into()),
                    options: vec!["fr-FR".into(), "en-US".into(), "es-ES".into(), "de-DE".into()],
                },
                ConfigField {
                    key: "base_url".into(),
                    label: "URL de base personnalisée".into(),
                    field_type: "text".into(),
                    description: "URL de base alternative pour proxy ou serveur de test mock".into(),
                    required: false,
                    secret: false,
                    default_value: None,
                    options: vec![],
                },
            ],
        }
    }

    async fn search(
        &self,
        query: SearchQuery,
        config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        let normalized = self.normalize_query(&query.title);
        if normalized.is_empty() {
            return Ok(Vec::new());
        }

        let first_char = normalized.chars().next().unwrap_or('a');
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_SUGGESTION_URL)
            .trim_end_matches('/');

        let url = format!("{base_url}/{first_char}/{normalized}.json");
        let text = self.fetch_text(&url, config).await?;

        let data: ImdbSuggestionResponse = serde_json::from_str(&text)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;

        let is_tv_query = matches!(query.media_type, MediaType::TvShow | MediaType::Episode);

        let items = data
            .d
            .into_iter()
            .filter_map(|item| {
                if !item.id.starts_with("tt") {
                    return None;
                }

                let q_type = item.q.as_deref().unwrap_or("").to_lowercase();
                let is_series = q_type.contains("series") || q_type.contains("tv");
                let media_type = if is_series {
                    MediaType::TvShow
                } else {
                    MediaType::Movie
                };

                let poster_url = item.i.map(|img| img.image_url);

                Some(SearchResultItem {
                    id: item.id,
                    title: item.l,
                    original_title: None,
                    year: item.y,
                    media_type,
                    overview: item.s.map(|cast| format!("Avec : {cast}")),
                    poster_url,
                    score: None,
                })
            })
            .filter(|item| {
                if let Some(target_year) = query.year {
                    if let Some(y) = item.year {
                        // Allow small 1-year tolerance for release date discrepancies
                        return (y as i32 - target_year as i32).abs() <= 1;
                    }
                }
                true
            })
            .filter(|item| {
                // If user specifically requested TV show, prioritize TV show
                if is_tv_query {
                    item.media_type == MediaType::TvShow
                } else {
                    true
                }
            })
            .collect();

        Ok(items)
    }

    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        let imdb_id = if request.id.starts_with("tt") {
            request.id.clone()
        } else if let Some(id) = request.external_ids.get("imdb") {
            id.clone()
        } else {
            return Err(PluginError::new(
                "invalid_imdb_id",
                format!(
                    "ID '{}' is not a valid IMDb ID (must start with 'tt')",
                    request.id
                ),
            ));
        };

        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_TITLE_URL)
            .trim_end_matches('/');

        let url = format!("{base_url}/{imdb_id}/");
        let html = self.fetch_text(&url, config).await?;

        // Extract Schema.org JSON-LD from HTML: <script type="application/ld+json">...</script>
        let json_ld_start = html
            .find(r#"type="application/ld+json">"#)
            .or_else(|| html.find(r#"type="application/ld+json" >"#))
            .or_else(|| html.find(r#"type='application/ld+json'>"#));

        let ld_text = if let Some(start_idx) = json_ld_start {
            let rest = &html[start_idx..];
            let content_start = rest.find('>').map(|i| i + 1).unwrap_or(0);
            let content_end = rest[content_start..].find("</script>").unwrap_or(0);
            &rest[content_start..content_start + content_end]
        } else {
            return Err(PluginError::new(
                "parse_error",
                "No JSON-LD metadata found on IMDb page",
            ));
        };

        let ld: ImdbJsonLd = serde_json::from_str(ld_text).map_err(|e| {
            PluginError::new("parse_error", format!("Failed to parse IMDb JSON-LD: {e}"))
        })?;

        let mut external_ids = BTreeMap::new();
        external_ids.insert("imdb".into(), imdb_id.clone());

        let year = ld
            .date_published
            .as_deref()
            .and_then(|d| d.split('-').next())
            .and_then(|y| y.parse::<u16>().ok());

        let runtime_minutes = ld
            .duration
            .as_deref()
            .and_then(|d| self.parse_iso_duration(d));

        let mut ratings = Vec::new();
        if let Some(agg) = ld.aggregate_rating {
            ratings.push(ProviderRating {
                provider: "imdb".into(),
                value: agg.rating_value,
                max: agg.best_rating.unwrap_or(10.0),
                votes: agg.rating_count,
            });
        }

        let directors: Vec<String> = ld.director.into_iter().map(|p| p.name).collect();

        let writers: Vec<String> = ld
            .creator
            .into_iter()
            .filter(|p| p.type_field.as_deref() == Some("Person"))
            .map(|p| p.name)
            .collect();

        let actors: Vec<ActorCredit> = ld
            .actor
            .into_iter()
            .enumerate()
            .map(|(idx, p)| ActorCredit {
                name: p.name,
                role: None,
                thumb_url: None,
                order: Some(idx as u32),
            })
            .collect();

        let mut posters = Vec::new();
        let mut images = Vec::new();
        if let Some(img_url) = ld.image {
            let img = MediaImage {
                url: img_url,
                image_type: ImageType::Poster,
                width: None,
                height: None,
                language: None,
                rating: None,
                vote_count: None,
                is_primary: true,
            };
            posters.push(img.clone());
            images.push(img);
        }

        Ok(StructuredMetadata {
            provider: "imdb".into(),
            provider_id: imdb_id,
            external_ids,
            title: Some(ld.name),
            original_title: ld.alternate_name,
            tagline: None,
            synopsis: ld.description,
            year,
            release_date: ld.date_published,
            runtime_minutes,
            genres: ld.genre.into_iter().collect(),
            directors,
            writers,
            actors,
            ratings,
            mpaa: ld.content_rating,
            status: None,
            posters,
            backdrops: Vec::new(),
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

// IMDb JSON models for suggestions
#[derive(Deserialize)]
struct ImdbSuggestionResponse {
    #[serde(default)]
    d: Vec<ImdbSuggestionItem>,
}

#[derive(Deserialize)]
struct ImdbSuggestionItem {
    id: String,
    l: String,
    s: Option<String>,
    y: Option<u16>,
    q: Option<String>,
    i: Option<ImdbSuggestionImage>,
}

#[derive(Deserialize)]
struct ImdbSuggestionImage {
    #[serde(rename = "imageUrl")]
    image_url: String,
}

// IMDb JSON-LD model
#[derive(Deserialize)]
struct ImdbJsonLd {
    name: String,
    #[serde(rename = "alternateName")]
    alternate_name: Option<String>,
    description: Option<String>,
    image: Option<String>,
    #[serde(rename = "datePublished")]
    date_published: Option<String>,
    duration: Option<String>,
    #[serde(rename = "contentRating")]
    content_rating: Option<String>,
    #[serde(default)]
    genre: OneOrMany<String>,
    #[serde(default)]
    actor: OneOrMany<ImdbPerson>,
    #[serde(default)]
    director: OneOrMany<ImdbPerson>,
    #[serde(default)]
    creator: OneOrMany<ImdbPerson>,
    #[serde(rename = "aggregateRating")]
    aggregate_rating: Option<ImdbAggregateRating>,
}

#[derive(Deserialize)]
struct ImdbPerson {
    #[serde(rename = "@type")]
    type_field: Option<String>,
    name: String,
}

#[derive(Deserialize)]
struct ImdbAggregateRating {
    #[serde(rename = "ratingValue")]
    rating_value: f64,
    #[serde(rename = "bestRating")]
    best_rating: Option<f64>,
    #[serde(rename = "ratingCount")]
    rating_count: Option<u64>,
}

/// Helper for JSON-LD fields that can be either a single element or an array
#[derive(Default, Deserialize)]
#[serde(untagged)]
enum OneOrMany<T> {
    #[default]
    None,
    One(T),
    Many(Vec<T>),
}

impl<T> IntoIterator for OneOrMany<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        match self {
            OneOrMany::None => Vec::new().into_iter(),
            OneOrMany::One(item) => vec![item].into_iter(),
            OneOrMany::Many(vec) => vec.into_iter(),
        }
    }
}

// Lightweight synchronous runner for guest WebAssembly execution
fn block_on_guest<F: std::future::Future>(future: F) -> F::Output {
    let raw_waker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut cx = Context::from_waker(&waker);
    let mut pinned = std::pin::pin!(future);
    match pinned.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("Guest Wasm async operation did not complete synchronously"),
    }
}
static VTABLE: RawWakerVTable =
    RawWakerVTable::new(|p| RawWaker::new(p, &VTABLE), |_| {}, |_| {}, |_| {});

media_extension_sdk::extension!(handle_imdb_extension_request);

pub fn handle_imdb_extension_request(
    req: media_extension_sdk::ExtensionRequest,
) -> media_extension_sdk::ExtensionResponse {
    let plugin = ImdbPlugin::new();
    let config = ProviderConfig::from_extension_input(&req.input);

    let result_json = match req.operation.as_str() {
        "manifest" => serde_json::to_value(plugin.manifest()).unwrap_or_default(),
        "metadata.search" | "search" => {
            let Ok(q) = serde_json::from_value::<SearchQuery>(req.input) else {
                return media_extension_sdk::ExtensionResponse {
                    api_version: 1,
                    result: serde_json::json!({"error": "invalid_search_input"}),
                };
            };
            match block_on_guest(plugin.search(q, &config)) {
                Ok(items) => serde_json::to_value(items).unwrap_or_default(),
                Err(e) => serde_json::json!({"error": e.code, "message": e.message}),
            }
        }
        "metadata.details" | "details" => {
            let Ok(d) = serde_json::from_value::<DetailsRequest>(req.input) else {
                return media_extension_sdk::ExtensionResponse {
                    api_version: 1,
                    result: serde_json::json!({"error": "invalid_details_input"}),
                };
            };
            match block_on_guest(plugin.details(d, &config)) {
                Ok(meta) => serde_json::to_value(meta).unwrap_or_default(),
                Err(e) => serde_json::json!({"error": e.code, "message": e.message}),
            }
        }
        "metadata.images" | "images" => {
            let Ok(i) = serde_json::from_value::<ImagesRequest>(req.input) else {
                return media_extension_sdk::ExtensionResponse {
                    api_version: 1,
                    result: serde_json::json!({"error": "invalid_images_input"}),
                };
            };
            match block_on_guest(plugin.images(i, &config)) {
                Ok(imgs) => serde_json::to_value(imgs).unwrap_or_default(),
                Err(e) => serde_json::json!({"error": e.code, "message": e.message}),
            }
        }
        "metadata.propose" => {
            let title = req
                .input
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let query = SearchQuery {
                title,
                year: req
                    .input
                    .get("year")
                    .and_then(|v| v.as_u64())
                    .and_then(|v| u16::try_from(v).ok()),
                media_type: req
                    .input
                    .get("mediaType")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or(MediaType::Movie),
                language: config.language.clone().or_else(|| Some("fr-FR".into())),
                external_ids: req
                    .input
                    .get("externalIds")
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default(),
            };
            match block_on_guest(plugin.search(query, &config)) {
                Ok(items) => {
                    if items.len() == 1 {
                        let first = &items[0];
                        let proposal = MetadataProposal {
                            title: first.title.clone(),
                            summary: first.overview.clone().unwrap_or_default(),
                        };
                        serde_json::to_value(proposal).unwrap_or_default()
                    } else {
                        serde_json::json!({"error": if items.is_empty() { "not_found" } else { "ambiguous_match" }})
                    }
                }
                Err(e) => serde_json::json!({"error": e.code, "message": e.message}),
            }
        }
        _ => serde_json::json!({"error": "unsupported_operation"}),
    };

    media_extension_sdk::ExtensionResponse {
        api_version: 1,
        result: result_json,
    }
}
