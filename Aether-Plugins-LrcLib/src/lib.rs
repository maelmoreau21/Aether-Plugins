//! LRCLIB lyrics provider extension for Aether (<https://lrclib.net>).
//!
//! Lyrics use the SDK lyrics.get contract (Lyrics, LyricsRequest), in addition to:
//! * the standard `search`/`details` operations (plain lyrics in `synopsis`), and
//! * a dedicated `lyrics.get` operation returning plain **and** time-synced lyrics.
//!
//! No API key is needed. Only lyrics text is fetched; no audio is handled.

use aether_plugin_api::http::{get_json, urlencode};
use aether_plugin_api::{
    ConfigField, DetailsRequest, ImagesRequest, LyricLine, Lyrics, LyricsRequest, MediaImage,
    MediaType, MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest,
    PluginRequest, PluginResponse, ProviderConfig, SearchQuery, SearchResultItem,
    StructuredMetadata, handle_request,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;

const DEFAULT_BASE_URL: &str = "https://lrclib.net/api";
const USER_AGENT: &str =
    "Aether-Plugin-LrcLib/0.1.0 (https://github.com/maelmoreau21/Aether-Plugins)";

#[derive(Clone, Default)]
pub struct LrcLibPlugin;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiLyrics {
    id: u64,
    track_name: Option<String>,
    artist_name: Option<String>,
    album_name: Option<String>,
    duration: Option<f64>,
    #[serde(default)]
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
}

/// Parse LRC text. Supports several timestamps per line and ignores metadata tags
/// such as `[ar:Artist]`. Result is sorted by time.
pub fn parse_lrc(lrc: &str) -> Vec<LyricLine> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some((tag, after)) = stripped.split_once(']') else {
                break;
            };
            match parse_timestamp(tag) {
                Some(ms) => times.push(ms),
                None if times.is_empty() => break,
                None => {}
            }
            rest = after;
        }
        let text = rest.trim().to_string();
        for time_ms in times {
            lines.push(LyricLine {
                time_ms,
                text: text.clone(),
            });
        }
    }
    lines.sort_by_key(|l| l.time_ms);
    lines
}

fn parse_timestamp(tag: &str) -> Option<u64> {
    let (min, sec) = tag.split_once(':')?;
    let min: u64 = min.parse().ok()?;
    let (whole, frac) = match sec.split_once('.') {
        Some((w, f)) => (w, f),
        None => (sec, ""),
    };
    let whole: u64 = whole.parse().ok()?;
    if whole >= 60 || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let millis = match frac.len() {
        0 => 0,
        1 => frac.parse::<u64>().ok()? * 100,
        2 => frac.parse::<u64>().ok()? * 10,
        _ => frac.get(..3)?.parse::<u64>().ok()?,
    };
    Some((min * 60 + whole) * 1000 + millis)
}

impl From<ApiLyrics> for Lyrics {
    fn from(a: ApiLyrics) -> Self {
        let synced = a.synced_lyrics.filter(|s| !s.trim().is_empty());
        Lyrics {
            id: a.id,
            track_name: a.track_name.unwrap_or_default(),
            artist_name: a.artist_name.unwrap_or_default(),
            album_name: a.album_name.filter(|s| !s.is_empty()),
            duration_secs: a.duration,
            instrumental: a.instrumental,
            plain: a.plain_lyrics.filter(|s| !s.trim().is_empty()),
            synced_lines: synced.as_deref().map(parse_lrc).unwrap_or_default(),
            synced,
        }
    }
}

impl LrcLibPlugin {
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

    async fn fetch(
        url: &str,
        config: &ProviderConfig,
        what: &str,
    ) -> Result<serde_json::Value, PluginError> {
        get_json(url, &[("User-Agent", USER_AGENT)], config, "LRCLIB", what).await
    }

    /// Fetch lyrics by LRCLIB id, or by track/artist (exact match first, then search fallback).
    pub async fn get_lyrics(
        &self,
        request: LyricsRequest,
        config: &ProviderConfig,
    ) -> Result<Lyrics, PluginError> {
        let base = Self::base_url(config);
        if let Some(id) = request.id {
            let value = Self::fetch(&format!("{base}/get/{id}"), config, &id.to_string()).await?;
            return parse_one(value);
        }
        if request.track.trim().is_empty() {
            return Err(PluginError::new(
                "missing_track",
                "A track name or an id is required",
            ));
        }
        let mut url = format!(
            "{base}/get?track_name={}&artist_name={}",
            urlencode(&request.track),
            urlencode(&request.artist)
        );
        if let Some(album) = request.album.as_deref().filter(|a| !a.is_empty()) {
            url.push_str(&format!("&album_name={}", urlencode(album)));
        }
        if let Some(duration) = request.duration_secs {
            url.push_str(&format!("&duration={duration}"));
        }
        match Self::fetch(&url, config, &request.track).await {
            Ok(value) => parse_one(value),
            Err(e) if e.code == "not_found" => {
                let results = self
                    .search_raw(&request.track, &request.artist, config)
                    .await?;
                results
                    .into_iter()
                    .next()
                    .ok_or_else(|| PluginError::not_found(&request.track))
            }
            Err(e) => Err(e),
        }
    }

    async fn search_raw(
        &self,
        track: &str,
        artist: &str,
        config: &ProviderConfig,
    ) -> Result<Vec<Lyrics>, PluginError> {
        let mut url = format!(
            "{}/search?track_name={}",
            Self::base_url(config),
            urlencode(track)
        );
        if !artist.trim().is_empty() {
            url.push_str(&format!("&artist_name={}", urlencode(artist)));
        }
        let value = Self::fetch(&url, config, track).await?;
        let list: Vec<ApiLyrics> = serde_json::from_value(value)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;
        Ok(list.into_iter().map(Lyrics::from).collect())
    }
}

fn parse_one(value: serde_json::Value) -> Result<Lyrics, PluginError> {
    serde_json::from_value::<ApiLyrics>(value)
        .map(Lyrics::from)
        .map_err(|e| PluginError::new("parse_error", e.to_string()))
}

#[async_trait]
impl MetadataProviderPlugin for LrcLibPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            permissions: vec![
                "metadata.read".into(),
                "metadata.write".into(),
                "images".into(),
            ],
            id: "lrclib".into(),
            name: "LRCLIB".into(),
            version: "0.1.0".into(),
            category: "Lyrics".into(),
            provider: "LRCLIB".into(),
            http_hosts: vec!["lrclib.net".into()],
            description: "Paroles simples et synchronisées (LRC) depuis lrclib.net, sans clé API"
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
                PluginCapability::Lyrics,
            ],
            supported_media_types: vec![MediaType::Music],
            default_priority: 50,
            config_schema: vec![ConfigField {
                key: "base_url".into(),
                label: "Adresse du fournisseur".into(),
                field_type: "text".into(),
                description: "Instance LRCLIB auto-hébergée (par défaut https://lrclib.net/api)"
                    .into(),
                required: false,
                secret: false,
                default_value: Some(DEFAULT_BASE_URL.into()),
                options: vec![],
            }],
        }
    }

    async fn search(
        &self,
        query: SearchQuery,
        config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        let artist = query
            .external_ids
            .get("artist")
            .cloned()
            .unwrap_or_default();
        Ok(self
            .search_raw(&query.title, &artist, config)
            .await?
            .into_iter()
            .map(|l| SearchResultItem {
                id: l.id.to_string(),
                title: l.track_name,
                original_title: None,
                year: None,
                media_type: query.media_type,
                overview: Some(match &l.album_name {
                    Some(album) => format!("{} — {album}", l.artist_name),
                    None => l.artist_name,
                }),
                poster_url: None,
                score: None,
            })
            .collect())
    }

    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        let id: u64 = request
            .id
            .parse()
            .map_err(|_| PluginError::new("invalid_id", "Expected a numeric LRCLIB id"))?;
        let l = self
            .get_lyrics(
                LyricsRequest {
                    id: Some(id),
                    ..Default::default()
                },
                config,
            )
            .await?;
        let mut external_ids = BTreeMap::new();
        external_ids.insert("lrclib".to_string(), l.id.to_string());
        Ok(StructuredMetadata {
            provider: "lrclib".into(),
            provider_id: l.id.to_string(),
            external_ids,
            title: Some(l.track_name),
            runtime_minutes: l.duration_secs.map(|d| (d / 60.0).round() as u32),
            synopsis: l.plain,
            tagline: Some(l.artist_name),
            ..Default::default()
        })
    }

    async fn images(
        &self,
        _request: ImagesRequest,
        _config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        Ok(Vec::new())
    }
}

/// Like [`handle_request`], with the additional `lyrics.get` operation.
pub async fn handle_lyrics_request(plugin: &LrcLibPlugin, raw_json: &str) -> String {
    let Ok(req) = serde_json::from_str::<PluginRequest>(raw_json) else {
        return handle_request(plugin, raw_json).await;
    };
    if req.operation != "lyrics.get" || req.api_version != aether_plugin_api::API_VERSION {
        return handle_request(plugin, raw_json).await;
    }
    let outcome = match serde_json::from_value::<LyricsRequest>(req.input) {
        Ok(r) => plugin.get_lyrics(r, &req.config).await,
        Err(e) => Err(PluginError::new("invalid_lyrics_input", e.to_string())),
    };
    let json = match outcome {
        Ok(l) => serde_json::to_string(&PluginResponse::ok(l)),
        Err(e) => serde_json::to_string(&PluginResponse::<Lyrics>::err(e)),
    };
    json.unwrap_or_else(|_| "{}".into())
}

media_extension_sdk::extension!(handle_lrclib_extension_request);

pub fn handle_lrclib_extension_request(
    req: aether_plugin_api::ExtensionRequest,
) -> aether_plugin_api::ExtensionResponse {
    if req.operation == "lyrics.get" {
        let config = ProviderConfig::from_extension_input(&req.input);
        let result = match serde_json::from_value::<LyricsRequest>(req.input) {
            Ok(r) => match aether_plugin_api::http::block_on_guest(
                LrcLibPlugin::new().get_lyrics(r, &config),
            ) {
                Ok(l) => serde_json::to_value(l).unwrap_or_default(),
                Err(e) => serde_json::json!({"error": e.code, "message": e.message}),
            },
            Err(_) => serde_json::json!({"error": "invalid_lyrics_input"}),
        };
        return aether_plugin_api::ExtensionResponse {
            api_version: aether_plugin_api::API_VERSION,
            result,
        };
    }
    aether_plugin_api::http::handle_extension(&LrcLibPlugin::new(), req)
}
