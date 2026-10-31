//! OMDb (Open Movie Database) metadata provider extension for Aether.

use aether_plugin_api::http::{get_json, urlencode};
use aether_plugin_api::{
    ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, ProviderConfig,
    ProviderRating, SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;

const DEFAULT_BASE_URL: &str = "https://www.omdbapi.com";
const USER_AGENT: &str = "Aether-Plugin-OMDb/0.1.0 (Aether Media Server)";

#[derive(Clone, Default)]
pub struct OmdbPlugin;

/// OMDb answers `N/A` for unknown values.
fn known(value: Option<String>) -> Option<String> {
    value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v != "N/A")
}

fn split_list(value: Option<String>) -> Vec<String> {
    known(value)
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Extract the first four-digit year from values such as `2014` or `2011–2019`.
fn parse_year(value: &str) -> Option<u16> {
    let digits: String = value
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.len() == 4 {
        digits.parse().ok()
    } else {
        None
    }
}

fn parse_votes(value: &str) -> Option<u64> {
    value.replace([',', ' '], "").parse().ok()
}

fn media_type_of(kind: &str) -> MediaType {
    match kind {
        "series" => MediaType::TvShow,
        "episode" => MediaType::Episode,
        _ => MediaType::Movie,
    }
}

impl OmdbPlugin {
    pub fn new() -> Self {
        Self
    }

    fn resolve_api_key(config: &ProviderConfig) -> Result<&str, PluginError> {
        config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| PluginError::missing_api_key("OMDb"))
    }

    fn base_url(config: &ProviderConfig) -> &str {
        config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
    }

    async fn call(
        url: &str,
        config: &ProviderConfig,
        what: &str,
    ) -> Result<serde_json::Value, PluginError> {
        let value = get_json(url, &[("User-Agent", USER_AGENT)], config, "OMDb", what).await?;
        if value.get("Response").and_then(|v| v.as_str()) == Some("False") {
            let message = value
                .get("Error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown OMDb error");
            return Err(match message {
                "Invalid API key!" => PluginError::new("unauthorized", message),
                "Request limit reached!" => PluginError::rate_limited(3600),
                _ if message.contains("not found") => PluginError::not_found(what),
                _ => PluginError::new("provider_error", message),
            });
        }
        Ok(value)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OmdbSearchEntry {
    title: String,
    year: Option<String>,
    #[serde(rename = "imdbID")]
    imdb_id: String,
    #[serde(rename = "Type")]
    kind: Option<String>,
    poster: Option<String>,
}

#[derive(Deserialize)]
struct OmdbSearchResponse {
    #[serde(rename = "Search", default)]
    search: Vec<OmdbSearchEntry>,
}

#[derive(Deserialize)]
struct OmdbRating {
    #[serde(rename = "Source")]
    source: String,
    #[serde(rename = "Value")]
    value: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct OmdbDetails {
    title: Option<String>,
    year: Option<String>,
    rated: Option<String>,
    released: Option<String>,
    runtime: Option<String>,
    genre: Option<String>,
    director: Option<String>,
    writer: Option<String>,
    actors: Option<String>,
    plot: Option<String>,
    poster: Option<String>,
    #[serde(default)]
    ratings: Vec<OmdbRating>,
    #[serde(rename = "imdbRating")]
    imdb_rating: Option<String>,
    #[serde(rename = "imdbVotes")]
    imdb_votes: Option<String>,
    #[serde(rename = "imdbID")]
    imdb_id: Option<String>,
}

/// Convert OMDb rating strings (`8.7/10`, `87%`, `74/100`) to value and maximum.
fn parse_rating(source: &str, raw: &str) -> Option<(f64, f64)> {
    let raw = raw.trim();
    if let Some(p) = raw.strip_suffix('%') {
        return p.trim().parse().ok().map(|v| (v, 100.0));
    }
    let (v, m) = raw.split_once('/')?;
    let _ = source;
    Some((v.trim().parse().ok()?, m.trim().parse().ok()?))
}

#[async_trait]
impl MetadataProviderPlugin for OmdbPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            permissions: vec![
                "metadata.read".into(),
                "metadata.write".into(),
                "images".into(),
            ],
            id: "omdb".into(),
            name: "OMDb".into(),
            version: "0.1.0".into(),
            category: "Metadata".into(),
            provider: "OMDb".into(),
            http_hosts: vec!["www.omdbapi.com".into()],
            description: "Métadonnées films et séries via l'Open Movie Database (clé API requise)"
                .into(),
            author: "Aether Team".into(),
            license: "AGPL-3.0-only".into(),
            target_abi: "1.0".into(),
            min_server_version: Some("0.1.0".into()),
            max_server_version: Some("1.0.0".into()),
            checksum: None,
            banner: Some(include_str!("../assets/banner.svg").into()),
            capabilities: vec![
                PluginCapability::Search,
                PluginCapability::Details,
                PluginCapability::Images,
            ],
            supported_media_types: vec![MediaType::Movie, MediaType::TvShow, MediaType::Episode],
            default_priority: 50,
            config_schema: vec![
                ConfigField {
                    key: "api_key".into(),
                    label: "Clé API OMDb".into(),
                    field_type: "password".into(),
                    description: "Clé gratuite ou Patreon obtenue sur omdbapi.com".into(),
                    required: true,
                    secret: true,
                    default_value: None,
                    options: vec![],
                },
                ConfigField {
                    key: "base_url".into(),
                    label: "Adresse du fournisseur".into(),
                    field_type: "text".into(),
                    description: "URL alternative pour proxy ou serveur de test".into(),
                    required: false,
                    secret: false,
                    default_value: Some(DEFAULT_BASE_URL.into()),
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
        let key = Self::resolve_api_key(config)?;
        let mut url = format!(
            "{}/?apikey={}&s={}",
            Self::base_url(config),
            urlencode(key),
            urlencode(&query.title)
        );
        if let Some(year) = query.year {
            url.push_str(&format!("&y={year}"));
        }
        match query.media_type {
            MediaType::Movie => url.push_str("&type=movie"),
            MediaType::TvShow | MediaType::Anime => url.push_str("&type=series"),
            MediaType::Episode => url.push_str("&type=episode"),
            MediaType::Music => {
                return Err(PluginError::new(
                    "unsupported_media_type",
                    "OMDb does not provide music metadata",
                ));
            }
        }
        let value = Self::call(&url, config, &query.title).await?;
        let parsed: OmdbSearchResponse = serde_json::from_value(value)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;
        Ok(parsed
            .search
            .into_iter()
            .map(|e| SearchResultItem {
                id: e.imdb_id,
                title: e.title,
                artist: None,
                original_title: None,
                year: e.year.as_deref().and_then(parse_year),
                media_type: media_type_of(e.kind.as_deref().unwrap_or("movie")),
                overview: None,
                poster_url: known(e.poster),
                score: None,
            })
            .collect())
    }

    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        let key = Self::resolve_api_key(config)?;
        let id = if request.id.trim().is_empty() {
            request
                .external_ids
                .get("imdb")
                .cloned()
                .unwrap_or_default()
        } else {
            request.id.clone()
        };
        if id.is_empty() {
            return Err(PluginError::new(
                "missing_id",
                "OMDb requires an IMDb identifier",
            ));
        }
        let url = format!(
            "{}/?apikey={}&i={}&plot=full",
            Self::base_url(config),
            urlencode(key),
            urlencode(&id)
        );
        let value = Self::call(&url, config, &id).await?;
        let d: OmdbDetails = serde_json::from_value(value)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;

        let mut ratings = Vec::new();
        if let Some(r) = known(d.imdb_rating).and_then(|r| r.parse::<f64>().ok()) {
            ratings.push(ProviderRating {
                provider: "imdb".into(),
                value: r,
                max: 10.0,
                votes: known(d.imdb_votes).and_then(|v| parse_votes(&v)),
            });
        }
        for r in d
            .ratings
            .iter()
            .filter(|r| r.source != "Internet Movie Database")
        {
            if let Some((value, max)) = parse_rating(&r.source, &r.value) {
                ratings.push(ProviderRating {
                    provider: r.source.to_lowercase().replace(' ', "-"),
                    value,
                    max,
                    votes: None,
                });
            }
        }

        let poster = known(d.poster).map(|url| MediaImage {
            url,
            image_type: ImageType::Poster,
            width: None,
            height: None,
            language: None,
            rating: None,
            vote_count: None,
            is_primary: true,
        });
        let provider_id = known(d.imdb_id).unwrap_or(id);
        let mut external_ids = BTreeMap::new();
        external_ids.insert("imdb".to_string(), provider_id.clone());

        Ok(StructuredMetadata {
            provider: "omdb".into(),
            provider_id,
            external_ids,
            title: known(d.title),
            year: d.year.as_deref().and_then(parse_year),
            release_date: known(d.released),
            runtime_minutes: known(d.runtime)
                .and_then(|r| r.split_whitespace().next().and_then(|n| n.parse().ok())),
            synopsis: known(d.plot),
            genres: split_list(d.genre),
            directors: split_list(d.director),
            writers: split_list(d.writer),
            actors: split_list(d.actors)
                .into_iter()
                .enumerate()
                .map(|(i, name)| aether_plugin_api::ActorCredit {
                    name,
                    role: None,
                    thumb_url: None,
                    order: Some(i as u32),
                })
                .collect(),
            ratings,
            mpaa: known(d.rated),
            images: poster.iter().cloned().collect(),
            posters: poster.into_iter().collect(),
            ..Default::default()
        })
    }

    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        Ok(self
            .details(
                DetailsRequest {
                    id: request.id,
                    media_type: request.media_type,
                    external_ids: request.external_ids,
                },
                config,
            )
            .await?
            .images)
    }
}

media_extension_sdk::extension!(handle_omdb_extension_request);

pub fn handle_omdb_extension_request(
    req: aether_plugin_api::ExtensionRequest,
) -> aether_plugin_api::ExtensionResponse {
    aether_plugin_api::http::handle_extension(&OmdbPlugin::new(), req)
}
