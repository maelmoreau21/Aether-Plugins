//! IMDb (Internet Movie Database) metadata provider extension for Aether.

use aether_plugin_api::{
    ActorCredit, ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, ProviderConfig,
    ProviderRating, SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

const DEFAULT_SUGGESTION_URL: &str = "https://v3.sg.media-imdb.com/suggestion";
const DEFAULT_TITLE_URL: &str = "https://www.imdb.com/title";

#[derive(Clone, Default)]
pub struct ImdbPlugin;

impl ImdbPlugin {
    pub fn new() -> Self {
        Self
    }

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
            provider: "imdb".into(),
            description: "Fournisseur de métadonnées IMDb (notes mondiales de référence, votes, casting et recherche instantanée)".into(),
            author: "Aether Team".into(),
            license: "MIT OR Apache-2.0".into(),
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
                    description: "Clé API optionnelle si vous utilisez un proxy IMDb / RapidAPI".into(),
                    required: false,
                    secret: true,
                    default_value: None,
                },
                ConfigField {
                    key: "language".into(),
                    label: "Langue préférée".into(),
                    description: "Code langue pour l'en-tête Accept-Language (ex. fr-FR, en-US)".into(),
                    required: false,
                    secret: false,
                    default_value: Some("fr-FR".into()),
                },
                ConfigField {
                    key: "base_url".into(),
                    label: "URL de base personnalisée".into(),
                    description: "URL de base alternative pour proxy ou serveur de test mock".into(),
                    required: false,
                    secret: false,
                    default_value: None,
                },
            ],
        }
    }

    async fn search(
        &self,
        query: SearchQuery,
        config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        let client = self.build_client(config)?;
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
        let res = client
            .get(&url)
            .send()
            .await
            .map_err(|e| PluginError::new("network_error", e.to_string()))?;

        if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(PluginError::rate_limited(10));
        }
        if !res.status().is_success() {
            return Err(PluginError::new(
                "provider_error",
                format!("IMDb suggestion API returned status {}", res.status()),
            ));
        }

        let data: ImdbSuggestionResponse = res
            .json()
            .await
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
        let client = self.build_client(config)?;
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
        let res = client
            .get(&url)
            .send()
            .await
            .map_err(|e| PluginError::new("network_error", e.to_string()))?;

        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(PluginError::not_found(&imdb_id));
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

        let html = res
            .text()
            .await
            .map_err(|e| PluginError::new("read_error", e.to_string()))?;

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
