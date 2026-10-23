//! TMDb (The Movie Database) metadata provider extension for Aether.
//! Operates in sandboxed WebAssembly (wasm32-unknown-unknown) with Aether Host HTTP broker,
//! and supports native execution for unit testing.

use aether_plugin_api::{
    ActorCredit, ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProposal, MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest,
    ProviderConfig, ProviderRating, SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

#[cfg(not(target_arch = "wasm32"))]
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://api.themoviedb.org/3";
const IMAGE_ORIGINAL_URL: &str = "https://image.tmdb.org/t/p/original";
const IMAGE_W500_URL: &str = "https://image.tmdb.org/t/p/w500";
const IMAGE_PROFILE_URL: &str = "https://image.tmdb.org/t/p/w185";

#[derive(Clone, Default)]
pub struct TmdbPlugin;

impl TmdbPlugin {
    pub fn new() -> Self {
        Self
    }

    fn resolve_api_key<'a>(&self, config: &'a ProviderConfig) -> Result<&'a str, PluginError> {
        if let Some(key) = config.api_key.as_deref().filter(|s| !s.trim().is_empty()) {
            return Ok(key.trim());
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(key) = std::env::var("TMDB_API_KEY") {
            if !key.trim().is_empty() {
                return Ok(Box::leak(key.into_boxed_str()));
            }
        }
        Err(PluginError::missing_api_key("tmdb"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn build_client(
        &self,
        config: &ProviderConfig,
        api_key: &str,
    ) -> Result<(reqwest::Client, String, bool), PluginError> {
        let timeout = Duration::from_secs(config.timeout_secs.unwrap_or(10));
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_string();

        let is_bearer = api_key.len() > 40 || api_key.starts_with("ey");
        let mut headers = HeaderMap::new();
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("Aether-Plugin-TMDb/0.1.0 (Aether Media Server)"),
        );
        if is_bearer {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
                headers.insert(AUTHORIZATION, val);
            }
        }

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .default_headers(headers)
            .build()
            .map_err(|e| PluginError::new("client_error", e.to_string()))?;

        Ok((client, base_url, is_bearer))
    }

    async fn fetch_json<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &[(&str, &str)],
        config: &ProviderConfig,
    ) -> Result<T, PluginError> {
        let api_key = self.resolve_api_key(config)?;
        let is_bearer = api_key.len() > 40 || api_key.starts_with("ey");
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/');

        let mut query_params = params.to_vec();
        if !is_bearer {
            query_params.push(("api_key", api_key));
        }

        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencoding::encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let full_url = if query_string.is_empty() {
            format!("{base_url}/{endpoint}")
        } else {
            format!("{base_url}/{endpoint}?{query_string}")
        };

        #[cfg(target_arch = "wasm32")]
        {
            let mut headers = BTreeMap::new();
            headers.insert("User-Agent".into(), "Aether-Plugin-TMDb/0.1.0".into());
            headers.insert("Accept".into(), "application/json".into());
            if is_bearer {
                headers.insert("Authorization".into(), format!("Bearer {api_key}"));
            }

            let host_req = media_extension_sdk::HostHttpRequest {
                url: full_url,
                method: "GET".into(),
                headers,
                body: None,
                timeout_secs: config.timeout_secs.map(|s| s as u32),
            };

            let resp = media_extension_sdk::host_http_fetch(&host_req)
                .map_err(|e| PluginError::new(e.code, e.message))?;

            if resp.status == 429 {
                return Err(PluginError::rate_limited(5));
            }
            if resp.status == 401 {
                return Err(PluginError::new("unauthorized", "Invalid TMDb API key"));
            }
            if resp.status != 200 {
                return Err(PluginError::new(
                    "provider_error",
                    format!("TMDb returned HTTP status {}", resp.status),
                ));
            }

            serde_json::from_slice(&resp.body)
                .map_err(|e| PluginError::new("parse_error", e.to_string()))
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let (client, _, _) = self.build_client(config, api_key)?;
            let req = client.get(&full_url);
            let res = req
                .send()
                .await
                .map_err(|e| PluginError::new("network_error", e.to_string()))?;

            if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(PluginError::rate_limited(5));
            }
            if res.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(PluginError::new("unauthorized", "Invalid TMDb API key"));
            }
            if !res.status().is_success() {
                return Err(PluginError::new(
                    "provider_error",
                    format!("TMDb returned status {}", res.status()),
                ));
            }

            res.json()
                .await
                .map_err(|e| PluginError::new("parse_error", e.to_string()))
        }
    }
}

#[async_trait]
impl MetadataProviderPlugin for TmdbPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            permissions: vec!["metadata.read".into(),"metadata.write".into(),"images".into()],
            id: "tmdb".into(),
            name: "TMDb (The Movie Database)".into(),
            version: "0.1.0".into(),
            category: "Metadata".into(),
            provider: "tmdb".into(),
            http_hosts: vec!["api.themoviedb.org".into(), "image.tmdb.org".into()],
            description: "Fournisseur officiel TMDb pour films et séries (métadonnées complètes, casting, affiches et notes)".into(),
            author: "Aether Team".into(),
            license: "MIT OR Apache-2.0".into(),
            target_abi: "1.0".into(),
            min_server_version: Some("0.1.0".into()),
            max_server_version: Some("1.0.0".into()),
            checksum: Some("f1a8c886111a8c9e504c55dfaa07a82fb89510b659cbe3544fdbb14243685e1b".into()),
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
            default_priority: 100,
            config_schema: vec![
                ConfigField {
                    key: "api_key".into(),
                    label: "Clé API TMDb".into(),
                    field_type: "password".into(),
                    description: "Clé API TMDb v3 ou jeton d'accès v4 Bearer (obtenue sur themoviedb.org)".into(),
                    required: true,
                    secret: true,
                    default_value: None,
                    options: vec![],
                },
                ConfigField {
                    key: "language".into(),
                    label: "Langue par défaut".into(),
                    field_type: "select".into(),
                    description: "Langue des titres et descriptions récupérés".into(),
                    required: false,
                    secret: false,
                    default_value: Some("fr-FR".into()),
                    options: vec!["fr-FR".into(), "en-US".into(), "es-ES".into(), "de-DE".into(), "ja-JP".into()],
                },
                ConfigField {
                    key: "include_adult".into(),
                    label: "Contenus adultes".into(),
                    field_type: "boolean".into(),
                    description: "Inclure les films et séries réservés aux adultes dans la recherche".into(),
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
        config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        let language = query
            .language
            .as_deref()
            .or(config.language.as_deref())
            .unwrap_or("fr-FR");

        let is_tv = matches!(query.media_type, MediaType::TvShow | MediaType::Episode);
        let endpoint = if is_tv { "search/tv" } else { "search/movie" };

        let mut params = vec![("query", query.title.as_str()), ("language", language)];

        let year_str = query.year.map(|y| y.to_string());
        if let Some(y) = &year_str {
            if is_tv {
                params.push(("first_air_date_year", y.as_str()));
            } else {
                params.push(("year", y.as_str()));
            }
        }

        let data: TmdbSearchResponse = self.fetch_json(endpoint, &params, config).await?;

        let items = data
            .results
            .into_iter()
            .map(|r| {
                let title = r.title.or(r.name).unwrap_or_else(|| "Sans titre".into());
                let original_title = r.original_title.or(r.original_name);
                let date_str = r.release_date.or(r.first_air_date);
                let year = date_str
                    .as_deref()
                    .and_then(|d| d.split('-').next())
                    .and_then(|y| y.parse::<u16>().ok());

                let poster_url = r
                    .poster_path
                    .as_deref()
                    .map(|p| format!("{IMAGE_W500_URL}{p}"));

                SearchResultItem {
                    id: r.id.to_string(),
                    title,
                    original_title,
                    year,
                    media_type: if is_tv {
                        MediaType::TvShow
                    } else {
                        MediaType::Movie
                    },
                    overview: r.overview.filter(|s| !s.trim().is_empty()),
                    poster_url,
                    score: r.vote_average,
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
        let language = config.language.as_deref().unwrap_or("fr-FR");
        let is_tv = matches!(request.media_type, MediaType::TvShow | MediaType::Episode);
        let endpoint = if is_tv {
            format!("tv/{}", request.id)
        } else {
            format!("movie/{}", request.id)
        };

        let params = [
            ("language", language),
            ("append_to_response", "credits,external_ids,images"),
        ];

        let val: serde_json::Value = self.fetch_json(&endpoint, &params, config).await?;

        let title = val
            .get("title")
            .or_else(|| val.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        let original_title = val
            .get("original_title")
            .or_else(|| val.get("original_name"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        let tagline = val
            .get("tagline")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned);

        let synopsis = val
            .get("overview")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned);

        let date_str = val
            .get("release_date")
            .or_else(|| val.get("first_air_date"))
            .and_then(|v| v.as_str());

        let year = date_str
            .and_then(|d| d.split('-').next())
            .and_then(|y| y.parse::<u16>().ok());

        let runtime = val
            .get("runtime")
            .and_then(|v| v.as_u64())
            .map(|r| r as u32)
            .or_else(|| {
                val.get("episode_run_time")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.as_u64())
                    .map(|r| r as u32)
            });

        let mut genres = Vec::new();
        if let Some(arr) = val.get("genres").and_then(|v| v.as_array()) {
            for g in arr {
                if let Some(name) = g.get("name").and_then(|v| v.as_str()) {
                    genres.push(name.to_owned());
                }
            }
        }

        let mut directors = Vec::new();
        let mut writers = Vec::new();
        let mut actors = Vec::new();

        if let Some(credits) = val.get("credits") {
            if let Some(crew) = credits.get("crew").and_then(|v| v.as_array()) {
                for c in crew {
                    let job = c.get("job").and_then(|v| v.as_str()).unwrap_or("");
                    let name = c.get("name").and_then(|v| v.as_str());
                    if let Some(n) = name {
                        if job == "Director" && !directors.contains(&n.to_string()) {
                            directors.push(n.to_string());
                        } else if (job == "Screenplay" || job == "Writer")
                            && !writers.contains(&n.to_string())
                        {
                            writers.push(n.to_string());
                        }
                    }
                }
            }

            if let Some(cast) = credits.get("cast").and_then(|v| v.as_array()) {
                for (idx, c) in cast.iter().take(20).enumerate() {
                    if let Some(name) = c.get("name").and_then(|v| v.as_str()) {
                        let role = c
                            .get("character")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned);
                        let thumb_url = c
                            .get("profile_path")
                            .and_then(|v| v.as_str())
                            .map(|p| format!("{IMAGE_PROFILE_URL}{p}"));
                        actors.push(ActorCredit {
                            name: name.to_string(),
                            role,
                            thumb_url,
                            order: Some(idx as u32),
                        });
                    }
                }
            }
        }

        let mut ratings = Vec::new();
        if let Some(vote_avg) = val.get("vote_average").and_then(|v| v.as_f64()) {
            let votes = val.get("vote_count").and_then(|v| v.as_u64());
            ratings.push(ProviderRating {
                provider: "tmdb".into(),
                value: vote_avg,
                max: 10.0,
                votes,
            });
        }

        let mut external_ids = BTreeMap::new();
        external_ids.insert("tmdb".into(), request.id.clone());
        if let Some(ext) = val.get("external_ids") {
            if let Some(imdb) = ext.get("imdb_id").and_then(|v| v.as_str()) {
                external_ids.insert("imdb".into(), imdb.to_string());
            }
            if let Some(tvdb) = ext.get("tvdb_id").and_then(|v| v.as_u64()) {
                external_ids.insert("tvdb".into(), tvdb.to_string());
            }
        }

        let mut posters = Vec::new();
        let mut backdrops = Vec::new();
        let mut all_images = Vec::new();

        if let Some(images_obj) = val.get("images") {
            if let Some(p_arr) = images_obj.get("posters").and_then(|v| v.as_array()) {
                for (idx, p) in p_arr.iter().enumerate() {
                    if let Some(file_path) = p.get("file_path").and_then(|v| v.as_str()) {
                        let img = MediaImage {
                            url: format!("{IMAGE_ORIGINAL_URL}{file_path}"),
                            image_type: ImageType::Poster,
                            width: p.get("width").and_then(|v| v.as_u64()).map(|w| w as u32),
                            height: p.get("height").and_then(|v| v.as_u64()).map(|h| h as u32),
                            language: p
                                .get("iso_639_1")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned),
                            rating: p.get("vote_average").and_then(|v| v.as_f64()),
                            vote_count: p.get("vote_count").and_then(|v| v.as_u64()),
                            is_primary: idx == 0,
                        };
                        posters.push(img.clone());
                        all_images.push(img);
                    }
                }
            }
            if let Some(b_arr) = images_obj.get("backdrops").and_then(|v| v.as_array()) {
                for (idx, b) in b_arr.iter().enumerate() {
                    if let Some(file_path) = b.get("file_path").and_then(|v| v.as_str()) {
                        let img = MediaImage {
                            url: format!("{IMAGE_ORIGINAL_URL}{file_path}"),
                            image_type: ImageType::Backdrop,
                            width: b.get("width").and_then(|v| v.as_u64()).map(|w| w as u32),
                            height: b.get("height").and_then(|v| v.as_u64()).map(|h| h as u32),
                            language: b
                                .get("iso_639_1")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned),
                            rating: b.get("vote_average").and_then(|v| v.as_f64()),
                            vote_count: b.get("vote_count").and_then(|v| v.as_u64()),
                            is_primary: idx == 0,
                        };
                        backdrops.push(img.clone());
                        all_images.push(img);
                    }
                }
            }
            if let Some(l_arr) = images_obj.get("logos").and_then(|v| v.as_array()) {
                for (idx, l) in l_arr.iter().enumerate() {
                    if let Some(file_path) = l.get("file_path").and_then(|v| v.as_str()) {
                        let img = MediaImage {
                            url: format!("{IMAGE_ORIGINAL_URL}{file_path}"),
                            image_type: ImageType::Logo,
                            width: l.get("width").and_then(|v| v.as_u64()).map(|w| w as u32),
                            height: l.get("height").and_then(|v| v.as_u64()).map(|h| h as u32),
                            language: l
                                .get("iso_639_1")
                                .and_then(|v| v.as_str())
                                .map(str::to_owned),
                            rating: l.get("vote_average").and_then(|v| v.as_f64()),
                            vote_count: l.get("vote_count").and_then(|v| v.as_u64()),
                            is_primary: idx == 0,
                        };
                        all_images.push(img);
                    }
                }
            }
        }

        if posters.is_empty() {
            if let Some(poster_path) = val.get("poster_path").and_then(|v| v.as_str()) {
                posters.push(MediaImage {
                    url: format!("{IMAGE_ORIGINAL_URL}{poster_path}"),
                    image_type: ImageType::Poster,
                    width: None,
                    height: None,
                    language: Some(language.into()),
                    rating: None,
                    vote_count: None,
                    is_primary: true,
                });
            }
        }
        if backdrops.is_empty() {
            if let Some(backdrop_path) = val.get("backdrop_path").and_then(|v| v.as_str()) {
                backdrops.push(MediaImage {
                    url: format!("{IMAGE_ORIGINAL_URL}{backdrop_path}"),
                    image_type: ImageType::Backdrop,
                    width: None,
                    height: None,
                    language: None,
                    rating: None,
                    vote_count: None,
                    is_primary: true,
                });
            }
        }

        Ok(StructuredMetadata {
            provider: "tmdb".into(),
            provider_id: request.id,
            external_ids,
            title,
            original_title,
            tagline,
            synopsis,
            year,
            release_date: date_str.map(str::to_owned),
            runtime_minutes: runtime,
            genres,
            directors,
            writers,
            actors,
            ratings,
            mpaa: None,
            status: val
                .get("status")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            posters,
            backdrops,
            images: all_images,
        })
    }

    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        let is_tv = matches!(request.media_type, MediaType::TvShow | MediaType::Episode);
        let endpoint = if is_tv {
            format!("tv/{}/images", request.id)
        } else {
            format!("movie/{}/images", request.id)
        };

        let val: serde_json::Value = self.fetch_json(&endpoint, &[], config).await?;
        let mut images = Vec::new();

        if let Some(posters) = val.get("posters").and_then(|v| v.as_array()) {
            for (idx, p) in posters.iter().enumerate() {
                if let Some(file_path) = p.get("file_path").and_then(|v| v.as_str()) {
                    images.push(MediaImage {
                        url: format!("{IMAGE_ORIGINAL_URL}{file_path}"),
                        image_type: ImageType::Poster,
                        width: p.get("width").and_then(|v| v.as_u64()).map(|w| w as u32),
                        height: p.get("height").and_then(|v| v.as_u64()).map(|h| h as u32),
                        language: p
                            .get("iso_639_1")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        rating: p.get("vote_average").and_then(|v| v.as_f64()),
                        vote_count: p.get("vote_count").and_then(|v| v.as_u64()),
                        is_primary: idx == 0,
                    });
                }
            }
        }

        if let Some(backdrops) = val.get("backdrops").and_then(|v| v.as_array()) {
            for (idx, b) in backdrops.iter().enumerate() {
                if let Some(file_path) = b.get("file_path").and_then(|v| v.as_str()) {
                    images.push(MediaImage {
                        url: format!("{IMAGE_ORIGINAL_URL}{file_path}"),
                        image_type: ImageType::Backdrop,
                        width: b.get("width").and_then(|v| v.as_u64()).map(|w| w as u32),
                        height: b.get("height").and_then(|v| v.as_u64()).map(|h| h as u32),
                        language: b
                            .get("iso_639_1")
                            .and_then(|v| v.as_str())
                            .map(str::to_owned),
                        rating: b.get("vote_average").and_then(|v| v.as_f64()),
                        vote_count: b.get("vote_count").and_then(|v| v.as_u64()),
                        is_primary: idx == 0,
                    });
                }
            }
        }

        Ok(images)
    }
}

#[derive(Deserialize)]
struct TmdbSearchResponse {
    results: Vec<TmdbSearchResult>,
}

#[derive(Deserialize)]
struct TmdbSearchResult {
    id: i64,
    title: Option<String>,
    name: Option<String>,
    original_title: Option<String>,
    original_name: Option<String>,
    overview: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    poster_path: Option<String>,
    vote_average: Option<f64>,
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

media_extension_sdk::extension!(handle_tmdb_extension_request);

pub fn handle_tmdb_extension_request(
    req: media_extension_sdk::ExtensionRequest,
) -> media_extension_sdk::ExtensionResponse {
    let plugin = TmdbPlugin::new();
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
