//! Discogs music metadata provider extension for Aether (releases and artwork).
//!
//! The Aether extension contract has no dedicated music media type yet: the plugin ignores
//! `media_type` and treats every query as a release (album/single) search.

use aether_plugin_api::http::{get_json, urlencode};
use aether_plugin_api::{
    ActorCredit, ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, ProviderConfig,
    ProviderRating, SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;

const DEFAULT_BASE_URL: &str = "https://api.discogs.com";
const USER_AGENT: &str =
    "Aether-Plugin-Discogs/0.1.0 +https://github.com/maelmoreau21/Aether-Plugins";

#[derive(Clone, Default)]
pub struct DiscogsPlugin;

#[derive(Deserialize)]
struct SearchEntry {
    id: u64,
    title: String,
    year: Option<String>,
    cover_image: Option<String>,
    thumb: Option<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SearchEntry>,
}

#[derive(Deserialize)]
struct Artist {
    name: String,
}

#[derive(Deserialize)]
struct Image {
    #[serde(rename = "type")]
    kind: Option<String>,
    uri: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Deserialize)]
struct Rating {
    average: Option<f64>,
    count: Option<u64>,
}

#[derive(Deserialize)]
struct Community {
    rating: Option<Rating>,
}

#[derive(Deserialize)]
struct Release {
    id: u64,
    title: String,
    year: Option<u16>,
    released: Option<String>,
    status: Option<String>,
    notes: Option<String>,
    #[serde(default)]
    artists: Vec<Artist>,
    #[serde(default)]
    genres: Vec<String>,
    #[serde(default)]
    styles: Vec<String>,
    #[serde(default)]
    images: Vec<Image>,
    community: Option<Community>,
}

fn nonempty(url: Option<String>) -> Option<String> {
    url.filter(|u| !u.trim().is_empty())
}

impl DiscogsPlugin {
    pub fn new() -> Self {
        Self
    }

    fn base_url(config: &ProviderConfig) -> &str {
        config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
    }

    fn token(config: &ProviderConfig) -> Option<&str> {
        config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    fn release_id(id: &str) -> Result<&str, PluginError> {
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            Ok(id)
        } else {
            Err(PluginError::new(
                "invalid_id",
                "Expected a numeric Discogs release id",
            ))
        }
    }

    async fn call(
        url: &str,
        config: &ProviderConfig,
        what: &str,
    ) -> Result<serde_json::Value, PluginError> {
        let auth = Self::token(config).map(|t| format!("Discogs token={t}"));
        let mut headers = vec![("User-Agent", USER_AGENT)];
        if let Some(auth) = auth.as_deref() {
            headers.push(("Authorization", auth));
        }
        get_json(url, &headers, config, "Discogs", what).await
    }

    async fn release(&self, id: &str, config: &ProviderConfig) -> Result<Release, PluginError> {
        let id = Self::release_id(id)?;
        let url = format!("{}/releases/{id}", Self::base_url(config));
        let value = Self::call(&url, config, id).await?;
        serde_json::from_value(value).map_err(|e| PluginError::new("parse_error", e.to_string()))
    }
}

fn to_images(images: Vec<Image>) -> Vec<MediaImage> {
    images
        .into_iter()
        .filter_map(|i| {
            let url = nonempty(i.uri)?;
            Some(MediaImage {
                url,
                image_type: ImageType::Poster,
                width: i.width,
                height: i.height,
                language: None,
                rating: None,
                vote_count: None,
                is_primary: i.kind.as_deref() == Some("primary"),
            })
        })
        .collect()
}

#[async_trait]
impl MetadataProviderPlugin for DiscogsPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            permissions: vec!["metadata.read".into(),"metadata.write".into(),"images".into()],
            id: "discogs".into(),
            name: "Discogs".into(),
            version: "0.1.0".into(),
            category: "Metadata".into(),
            provider: "Discogs".into(),
            http_hosts: vec!["api.discogs.com".into()],
            description: "MǸtadonnǸes d'albums (artistes, genres, styles, notes) et pochettes via Discogs".into(),
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
            supported_media_types: vec![MediaType::Music],
            default_priority: 55,
            config_schema: vec![
                ConfigField {
                    key: "api_key".into(),
                    label: "Jeton personnel Discogs".into(),
                    field_type: "password".into(),
                    description: "Jeton gǸnǸrǸ dans Discogs > Settings > Developers (obligatoire pour la recherche et les images)".into(),
                    required: true,
                    secret: true,
                    default_value: None,
                    options: vec![],
                },
                ConfigField {
                    key: "base_url".into(),
                    label: "URL de base personnalisǸe".into(),
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
        if Self::token(config).is_none() {
            return Err(PluginError::missing_api_key("Discogs"));
        }
        let mut url = format!(
            "{}/database/search?type=release&per_page=10&q={}",
            Self::base_url(config),
            urlencode(&query.title)
        );
        if let Some(artist) = query.external_ids.get("artist") {
            url.push_str(&format!("&artist={}", urlencode(artist)));
        }
        if let Some(year) = query.year {
            url.push_str(&format!("&year={year}"));
        }
        let value = Self::call(&url, config, &query.title).await?;
        let parsed: SearchResponse = serde_json::from_value(value)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;
        Ok(parsed
            .results
            .into_iter()
            .map(|e| {
                // Discogs titles are formatted "Artist - Album".
                let (artist, title) = match e.title.split_once(" - ") {
                    Some((a, t)) => (Some(a.to_string()), t.to_string()),
                    None => (None, e.title.clone()),
                };
                SearchResultItem {
                    id: e.id.to_string(),
                    title,
                    artist: artist.clone(),
                    original_title: None,
                    year: e.year.as_deref().and_then(|y| y.parse().ok()),
                    media_type: query.media_type,
                    overview: artist,
                    poster_url: nonempty(e.cover_image).or_else(|| nonempty(e.thumb)),
                    score: None,
                }
            })
            .collect())
    }

    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        let r = self.release(&request.id, config).await?;
        let mut external_ids = BTreeMap::new();
        external_ids.insert("discogs".to_string(), r.id.to_string());
        let ratings = r
            .community
            .and_then(|c| c.rating)
            .and_then(|r| {
                Some(ProviderRating {
                    provider: "discogs".into(),
                    value: r.average.filter(|v| *v > 0.0)?,
                    max: 5.0,
                    votes: r.count,
                })
            })
            .into_iter()
            .collect();
        let images = to_images(r.images);
        let mut genres = r.genres;
        genres.extend(r.styles);
        Ok(StructuredMetadata {
            provider: "discogs".into(),
            provider_id: r.id.to_string(),
            external_ids,
            title: Some(r.title),
            year: r.year.filter(|y| *y > 0),
            release_date: r.released,
            synopsis: r.notes.filter(|n| !n.trim().is_empty()),
            genres,
            actors: r
                .artists
                .into_iter()
                .enumerate()
                .map(|(i, a)| ActorCredit {
                    name: a.name,
                    role: Some("artist".into()),
                    thumb_url: None,
                    order: Some(i as u32),
                })
                .collect(),
            ratings,
            status: r.status,
            posters: images.clone(),
            images,
            ..Default::default()
        })
    }

    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        Ok(to_images(self.release(&request.id, config).await?.images))
    }
}

media_extension_sdk::extension!(handle_discogs_extension_request);

pub fn handle_discogs_extension_request(
    req: aether_plugin_api::ExtensionRequest,
) -> aether_plugin_api::ExtensionResponse {
    let search = matches!(req.operation.as_str(), "metadata.search" | "search");
    let mut response = aether_plugin_api::http::handle_extension(&DiscogsPlugin::new(), req);
    if search {
        aether_plugin_api::http::promote_music_artist(&mut response);
    }
    response
}
