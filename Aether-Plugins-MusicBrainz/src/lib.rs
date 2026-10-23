//! MusicBrainz music metadata provider extension for Aether (releases and cover art).
//!
//! The Aether extension contract has no dedicated music media type yet: the plugin ignores
//! `media_type` and treats every query as an album (MusicBrainz *release*) search. An
//! optional `artist` entry in `external_ids` narrows the search.

use aether_plugin_api::http::{get_json, throttle, urlencode};
use aether_plugin_api::{
    ActorCredit, ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, ProviderConfig,
    SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;

const DEFAULT_BASE_URL: &str = "https://musicbrainz.org/ws/2";
const DEFAULT_COVER_ART_URL: &str = "https://coverartarchive.org";
/// MusicBrainz allows at most one request per second per client.
const MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1100);

const DEFAULT_USER_AGENT: &str =
    "Aether-Plugin-MusicBrainz/0.1.0 (https://github.com/maelmoreau21/Aether-Plugins)";

#[derive(Clone, Default)]
pub struct MusicBrainzPlugin;

#[derive(Deserialize)]
struct ArtistCredit {
    name: String,
}

#[derive(Deserialize)]
struct ReleaseGroup {
    id: String,
}

#[derive(Deserialize)]
struct Genre {
    name: String,
}

#[derive(Deserialize)]
struct Track {
    length: Option<u64>,
}

#[derive(Deserialize)]
struct Medium {
    #[serde(default)]
    tracks: Vec<Track>,
}

#[derive(Deserialize)]
struct Release {
    id: String,
    title: String,
    date: Option<String>,
    status: Option<String>,
    disambiguation: Option<String>,
    score: Option<f64>,
    #[serde(rename = "artist-credit", default)]
    artist_credit: Vec<ArtistCredit>,
    #[serde(rename = "release-group")]
    release_group: Option<ReleaseGroup>,
    #[serde(default)]
    genres: Vec<Genre>,
    #[serde(default)]
    media: Vec<Medium>,
}

#[derive(Deserialize)]
struct SearchResponse {
    #[serde(default)]
    releases: Vec<Release>,
}

#[derive(Deserialize)]
struct CoverArt {
    #[serde(default)]
    images: Vec<CoverImage>,
}

#[derive(Deserialize)]
struct CoverImage {
    image: String,
    #[serde(default)]
    front: bool,
    #[serde(default)]
    back: bool,
}

fn year_of(date: &str) -> Option<u16> {
    date.get(..4).and_then(|y| y.parse().ok())
}

fn https(url: String) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url,
    }
}

/// Escape characters that are special in Lucene phrase queries.
fn lucene_phrase(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

impl MusicBrainzPlugin {
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

    /// Request spacing; `extra.min_interval_ms` overrides it (e.g. for a private mirror).
    fn min_interval(config: &ProviderConfig) -> std::time::Duration {
        config
            .extra
            .get("min_interval_ms")
            .and_then(|v| v.parse().ok())
            .map(std::time::Duration::from_millis)
            .unwrap_or(MIN_INTERVAL)
    }

    fn user_agent(config: &ProviderConfig) -> &str {
        config
            .extra
            .get("user_agent")
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(DEFAULT_USER_AGENT)
    }

    fn release_id(id: &str) -> Result<&str, PluginError> {
        let ok = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
        if ok {
            Ok(id)
        } else {
            Err(PluginError::new(
                "invalid_id",
                "Expected a MusicBrainz release MBID",
            ))
        }
    }
}

#[async_trait]
impl MetadataProviderPlugin for MusicBrainzPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            id: "musicbrainz".into(),
            name: "MusicBrainz".into(),
            version: "0.1.0".into(),
            category: "Metadata".into(),
            provider: "MusicBrainz".into(),
            http_hosts: vec!["musicbrainz.org".into(), "coverartarchive.org".into()],
            description: "MÃƒÂ©tadonnÃƒÂ©es d'albums (artistes, dates, genres) et pochettes via MusicBrainz et Cover Art Archive".into(),
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
            default_priority: 50,
            config_schema: vec![ConfigField {
                key: "base_url".into(),
                label: "URL de base personnalisÃƒÂ©e".into(),
                field_type: "text".into(),
                description: "Miroir MusicBrainz ou serveur de test (aucune clÃƒÂ© requise)".into(),
                required: false,
                secret: false,
                default_value: None,
                options: vec![],
            }],
        }
    }

    async fn search(
        &self,
        query: SearchQuery,
        config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        let mut lucene = format!("release:\"{}\"", lucene_phrase(&query.title));
        if let Some(artist) = query.external_ids.get("artist") {
            lucene.push_str(&format!(" AND artist:\"{}\"", lucene_phrase(artist)));
        }
        let url = format!(
            "{}/release?query={}&fmt=json&limit=10",
            Self::base_url(config),
            urlencode(&lucene)
        );
        throttle("musicbrainz", Self::min_interval(config)).await;
        let value = get_json(
            &url,
            &[("User-Agent", Self::user_agent(config))],
            config,
            "MusicBrainz",
            &query.title,
        )
        .await?;
        let parsed: SearchResponse = serde_json::from_value(value)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;
        Ok(parsed
            .releases
            .into_iter()
            .map(|r| {
                let artists: Vec<_> = r.artist_credit.iter().map(|a| a.name.as_str()).collect();
                SearchResultItem {
                    id: r.id,
                    title: r.title,
                    original_title: None,
                    year: r.date.as_deref().and_then(year_of),
                    media_type: query.media_type,
                    overview: (!artists.is_empty()).then(|| artists.join(", ")),
                    poster_url: None,
                    score: r.score,
                }
            })
            .collect())
    }

    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        let id = Self::release_id(&request.id)?;
        let url = format!(
            "{}/release/{id}?inc=artist-credits+genres+recordings+release-groups&fmt=json",
            Self::base_url(config)
        );
        throttle("musicbrainz", Self::min_interval(config)).await;
        let value = get_json(
            &url,
            &[("User-Agent", Self::user_agent(config))],
            config,
            "MusicBrainz",
            id,
        )
        .await?;
        let r: Release = serde_json::from_value(value)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;

        let total_ms: u64 = r
            .media
            .iter()
            .flat_map(|m| &m.tracks)
            .filter_map(|t| t.length)
            .sum();
        let mut external_ids = BTreeMap::new();
        external_ids.insert("musicbrainz_release".to_string(), r.id.clone());
        if let Some(group) = &r.release_group {
            external_ids.insert("musicbrainz_release_group".to_string(), group.id.clone());
        }
        Ok(StructuredMetadata {
            provider: "musicbrainz".into(),
            provider_id: r.id,
            external_ids,
            title: Some(r.title),
            year: r.date.as_deref().and_then(year_of),
            release_date: r.date,
            runtime_minutes: (total_ms > 0).then_some((total_ms / 60_000) as u32),
            genres: r.genres.into_iter().map(|g| g.name).collect(),
            actors: r
                .artist_credit
                .into_iter()
                .enumerate()
                .map(|(i, a)| ActorCredit {
                    name: a.name,
                    role: Some("artist".into()),
                    thumb_url: None,
                    order: Some(i as u32),
                })
                .collect(),
            synopsis: r.disambiguation.filter(|d| !d.is_empty()),
            status: r.status,
            ..Default::default()
        })
    }

    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        let id = Self::release_id(&request.id)?;
        let base = config
            .extra
            .get("cover_art_url")
            .map(String::as_str)
            .unwrap_or(DEFAULT_COVER_ART_URL)
            .trim_end_matches('/');
        let value = get_json(
            &format!("{base}/release/{id}"),
            &[("User-Agent", Self::user_agent(config))],
            config,
            "Cover Art Archive",
            id,
        )
        .await?;
        let art: CoverArt = serde_json::from_value(value)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;
        Ok(art
            .images
            .into_iter()
            .enumerate()
            .map(|(i, img)| MediaImage {
                url: https(img.image),
                image_type: if img.back {
                    ImageType::Thumb
                } else {
                    ImageType::Poster
                },
                width: None,
                height: None,
                language: None,
                rating: None,
                vote_count: None,
                is_primary: img.front || (i == 0 && !img.back),
            })
            .collect())
    }
}

media_extension_sdk::extension!(handle_musicbrainz_extension_request);

pub fn handle_musicbrainz_extension_request(
    req: aether_plugin_api::ExtensionRequest,
) -> aether_plugin_api::ExtensionResponse {
    aether_plugin_api::http::handle_extension(&MusicBrainzPlugin::new(), req)
}
