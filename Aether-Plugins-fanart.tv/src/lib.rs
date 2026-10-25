//! fanart.tv high-quality artwork and image provider extension for Aether.

use aether_plugin_api::{
    ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, ProviderConfig,
    SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
#[cfg(not(target_arch = "wasm32"))]
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

const DEFAULT_BASE_URL: &str = "https://webservice.fanart.tv/v3";

#[derive(Clone, Default)]
pub struct FanartTvPlugin;

impl FanartTvPlugin {
    pub fn new() -> Self {
        Self
    }

    fn resolve_api_key<'a>(&self, config: &'a ProviderConfig) -> Result<&'a str, PluginError> {
        if let Some(key) = config.api_key.as_deref().filter(|s| !s.trim().is_empty()) {
            return Ok(key.trim());
        }
        if let Ok(key) = std::env::var("FANART_TV_API_KEY") {
            if !key.trim().is_empty() {
                return Ok(Box::leak(key.into_boxed_str()));
            }
        }
        Err(PluginError::missing_api_key("fanart.tv"))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn build_client(
        &self,
        config: &ProviderConfig,
    ) -> Result<(reqwest::Client, String), PluginError> {
        let timeout = Duration::from_secs(config.timeout_secs.unwrap_or(10));
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/')
            .to_string();

        let mut headers = HeaderMap::new();
        headers.insert(
            "User-Agent",
            HeaderValue::from_static("Aether-Plugin-FanartTv/0.1.0 (Aether Media Server)"),
        );
        headers.insert("Accept", HeaderValue::from_static("application/json"));

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .default_headers(headers)
            .build()
            .map_err(|e| PluginError::new("client_error", e.to_string()))?;

        Ok((client, base_url))
    }

    async fn fetch_artwork_json(
        &self,
        endpoint: &str,
        api_key: &str,
        config: &ProviderConfig,
    ) -> Result<serde_json::Value, PluginError> {
        let base_url = config
            .base_url
            .as_deref()
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/');
        let url = format!("{base_url}/{endpoint}?api_key={api_key}");

        #[cfg(target_arch = "wasm32")]
        {
            let mut headers = BTreeMap::new();
            headers.insert("User-Agent".into(), "Aether-Plugin-FanartTv/0.1.0".into());
            headers.insert("Accept".into(), "application/json".into());
            if let Some(user_key) = config.user_key.as_deref().filter(|s| !s.trim().is_empty()) {
                headers.insert("user-key".into(), user_key.to_string());
            }

            let host_req = media_extension_sdk::HostHttpRequest {
                url,
                method: "GET".into(),
                headers,
                body: None,
                timeout_secs: config.timeout_secs.map(|s| s as u32),
            };

            let resp = media_extension_sdk::host_http_fetch(&host_req)
                .map_err(|e| PluginError::new(e.code, e.message))?;

            if resp.status == 404 {
                return Err(PluginError::not_found(endpoint));
            }
            if resp.status == 429 {
                return Err(PluginError::rate_limited(10));
            }
            if resp.status == 401 {
                return Err(PluginError::new(
                    "unauthorized",
                    "Invalid fanart.tv API key",
                ));
            }
            if resp.status != 200 {
                return Err(PluginError::new(
                    "provider_error",
                    format!("fanart.tv returned HTTP status {}", resp.status),
                ));
            }

            serde_json::from_slice(&resp.body)
                .map_err(|e| PluginError::new("parse_error", e.to_string()))
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let (client, _) = self.build_client(config)?;
            let mut req = client.get(&url);
            if let Some(user_key) = config.user_key.as_deref().filter(|s| !s.trim().is_empty()) {
                req = req.header("user-key", user_key);
            }

            let res = req
                .send()
                .await
                .map_err(|e| PluginError::new("network_error", e.to_string()))?;

            if res.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(PluginError::not_found(endpoint));
            }
            if res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(PluginError::rate_limited(10));
            }
            if res.status() == reqwest::StatusCode::UNAUTHORIZED {
                return Err(PluginError::new(
                    "unauthorized",
                    "Invalid fanart.tv API key",
                ));
            }
            if !res.status().is_success() {
                return Err(PluginError::new(
                    "provider_error",
                    format!("fanart.tv returned status {}", res.status()),
                ));
            }

            res.json()
                .await
                .map_err(|e| PluginError::new("parse_error", e.to_string()))
        }
    }

    fn convert_image_list(
        &self,
        items: Vec<FanartImageItem>,
        img_type: ImageType,
    ) -> Vec<MediaImage> {
        items
            .into_iter()
            .enumerate()
            .map(|(idx, item)| {
                let rating = item.likes.as_deref().and_then(|s| s.parse::<f64>().ok());
                let language = item.lang.filter(|l| l != "00" && !l.is_empty());
                MediaImage {
                    url: item.url,
                    image_type: img_type,
                    width: None,
                    height: None,
                    language,
                    rating,
                    vote_count: None,
                    is_primary: idx == 0,
                }
            })
            .collect()
    }
}

#[async_trait]
impl MetadataProviderPlugin for FanartTvPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            permissions: vec!["metadata.read".into(),"metadata.write".into(),"images".into()],
            id: "fanart-tv".into(),
            name: "fanart.tv".into(),
            version: "0.1.0".into(),
            category: "Artwork".into(),
            provider: "fanart.tv".into(),
            http_hosts: vec!["webservice.fanart.tv".into(), "assets.fanart.tv".into()],
            description: "Fournisseur d'images haute définition (ClearLogos, Backdrops 4K/HD, ClearArt, Bannières, Disques)".into(),
            author: "Aether Team".into(),
            license: "MIT OR Apache-2.0".into(),
            target_abi: "1.0".into(),
            min_server_version: Some("0.1.0".into()),
            max_server_version: Some("1.0.0".into()),
            checksum: Some("d3c7a884333b7a1f726e77fa019b884ec81732d871ecf5766afcb2646589a13d".into()),
            banner: Some(include_str!("../assets/banner.svg").into()),
            capabilities: vec![
                PluginCapability::Details,
                PluginCapability::Images,
            ],
            supported_media_types: vec![
                MediaType::Movie,
                MediaType::TvShow,
                MediaType::Episode,
                MediaType::Anime,
            ],
            default_priority: 60,
            config_schema: vec![
                ConfigField {
                    key: "api_key".into(),
                    label: "Clé API Projet (client key)".into(),
                    field_type: "password".into(),
                    description: "Clé API fanart.tv pour votre application (obtenue sur fanart.tv)".into(),
                    required: true,
                    secret: true,
                    default_value: None,
                    options: vec![],
                },
                ConfigField {
                    key: "user_key".into(),
                    label: "Clé Utilisateur VIP (optionnelle)".into(),
                    field_type: "password".into(),
                    description: "Clé personnelle VIP pour un accès prioritaire aux dernières contributions".into(),
                    required: false,
                    secret: true,
                    default_value: None,
                    options: vec![],
                },
                ConfigField {
                    key: "base_url".into(),
                    label: "URL de base personnalisée".into(),
                    field_type: "text".into(),
                    description: "URL alternative pour proxy ou serveur de test mock".into(),
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
        _config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        let id = if matches!(query.media_type, MediaType::TvShow | MediaType::Episode) {
            query.external_ids.get("tvdb")
        } else {
            query
                .external_ids
                .get("tmdb")
                .or_else(|| query.external_ids.get("imdb"))
        };
        if let Some(id) = id {
            return Ok(vec![SearchResultItem {
                id: id.clone(),
                title: query.title,
                original_title: None,
                year: query.year,
                media_type: query.media_type,
                overview: Some("Artwork indexed by the existing external identifier".into()),
                poster_url: None,
                score: None,
            }]);
        }
        // Direct title search is not supported by fanart.tv API
        Err(PluginError::new(
            "external_id_required",
            "An external media identifier is required",
        ))
    }

    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        let api_key = self.resolve_api_key(config)?;

        let is_tv = matches!(request.media_type, MediaType::TvShow | MediaType::Episode);
        let id = if !request.id.trim().is_empty() {
            request.id.clone()
        } else if is_tv {
            request
                .external_ids
                .get("tvdb")
                .cloned()
                .unwrap_or_default()
        } else {
            request
                .external_ids
                .get("tmdb")
                .or_else(|| request.external_ids.get("imdb"))
                .cloned()
                .unwrap_or_default()
        };
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(PluginError::new(
                "missing_id",
                if is_tv {
                    "fanart.tv TV artwork requires a TVDB identifier"
                } else {
                    "fanart.tv movie artwork requires a TMDb or IMDb identifier"
                },
            ));
        }

        let endpoint = if is_tv {
            format!("tv/{id}")
        } else {
            format!("movies/{id}")
        };

        let raw_val = self.fetch_artwork_json(&endpoint, api_key, config).await?;

        let mut posters = Vec::new();
        let mut backdrops = Vec::new();
        let mut all_images = Vec::new();
        let mut external_ids = BTreeMap::new();
        let title: Option<String>;

        if is_tv {
            let data: FanartTvShowResponse = serde_json::from_value(raw_val)
                .map_err(|e| PluginError::new("parse_error", e.to_string()))?;

            title = Some(data.name);
            if let Some(tvdb) = data.thetvdb_id {
                external_ids.insert("tvdb".into(), tvdb);
            }

            let p = self.convert_image_list(data.tvposter, ImageType::Poster);
            posters.extend(p.clone());
            all_images.extend(p);

            let sp = self.convert_image_list(data.seasonposter, ImageType::Poster);
            posters.extend(sp.clone());
            all_images.extend(sp);

            let bg = self.convert_image_list(data.showbackground, ImageType::Backdrop);
            backdrops.extend(bg.clone());
            all_images.extend(bg);

            all_images.extend(self.convert_image_list(data.hdtvlogo, ImageType::Logo));
            all_images.extend(self.convert_image_list(data.clearlogo, ImageType::Logo));
            all_images.extend(self.convert_image_list(data.clearart, ImageType::ClearArt));
            all_images.extend(self.convert_image_list(data.tvbanner, ImageType::Banner));
            all_images.extend(self.convert_image_list(data.tvthumb, ImageType::Thumb));
        } else {
            let data: FanartMovieResponse = serde_json::from_value(raw_val)
                .map_err(|e| PluginError::new("parse_error", e.to_string()))?;

            title = Some(data.name);
            if let Some(tmdb) = data.tmdb_id {
                external_ids.insert("tmdb".into(), tmdb);
            }
            if let Some(imdb) = data.imdb_id {
                external_ids.insert("imdb".into(), imdb);
            }

            let p = self.convert_image_list(data.movieposter, ImageType::Poster);
            posters.extend(p.clone());
            all_images.extend(p);

            let bg = self.convert_image_list(data.moviebackground, ImageType::Backdrop);
            backdrops.extend(bg.clone());
            all_images.extend(bg);

            all_images.extend(self.convert_image_list(data.hdmovielogo, ImageType::Logo));
            all_images.extend(self.convert_image_list(data.movielogo, ImageType::Logo));
            all_images.extend(self.convert_image_list(data.hdmovieart, ImageType::ClearArt));
            all_images.extend(self.convert_image_list(data.movieart, ImageType::ClearArt));
            all_images.extend(self.convert_image_list(data.moviebanner, ImageType::Banner));
            all_images.extend(self.convert_image_list(data.moviethumb, ImageType::Thumb));
            all_images.extend(self.convert_image_list(data.moviedisc, ImageType::DiscArt));
        }

        Ok(StructuredMetadata {
            provider: "fanart.tv".into(),
            provider_id: id,
            external_ids,
            title,
            original_title: None,
            tagline: None,
            synopsis: None,
            year: None,
            release_date: None,
            runtime_minutes: None,
            genres: Vec::new(),
            directors: Vec::new(),
            writers: Vec::new(),
            actors: Vec::new(),
            ratings: Vec::new(),
            mpaa: None,
            status: None,
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
        Ok(config.select_images(details.images))
    }
}

// fanart.tv JSON response models
#[derive(Deserialize)]
struct FanartImageItem {
    url: String,
    lang: Option<String>,
    likes: Option<String>,
}

#[derive(Deserialize)]
struct FanartMovieResponse {
    name: String,
    tmdb_id: Option<String>,
    imdb_id: Option<String>,
    #[serde(default)]
    movieposter: Vec<FanartImageItem>,
    #[serde(default)]
    moviebackground: Vec<FanartImageItem>,
    #[serde(default)]
    hdmovielogo: Vec<FanartImageItem>,
    #[serde(default)]
    movielogo: Vec<FanartImageItem>,
    #[serde(default)]
    hdmovieart: Vec<FanartImageItem>,
    #[serde(default)]
    movieart: Vec<FanartImageItem>,
    #[serde(default)]
    moviebanner: Vec<FanartImageItem>,
    #[serde(default)]
    moviethumb: Vec<FanartImageItem>,
    #[serde(default)]
    moviedisc: Vec<FanartImageItem>,
}

#[derive(Deserialize)]
struct FanartTvShowResponse {
    name: String,
    thetvdb_id: Option<String>,
    #[serde(default)]
    tvposter: Vec<FanartImageItem>,
    #[serde(default)]
    seasonposter: Vec<FanartImageItem>,
    #[serde(default)]
    showbackground: Vec<FanartImageItem>,
    #[serde(default)]
    hdtvlogo: Vec<FanartImageItem>,
    #[serde(default)]
    clearlogo: Vec<FanartImageItem>,
    #[serde(default)]
    clearart: Vec<FanartImageItem>,
    #[serde(default)]
    tvbanner: Vec<FanartImageItem>,
    #[serde(default)]
    tvthumb: Vec<FanartImageItem>,
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

media_extension_sdk::extension!(handle_fanart_extension_request);

pub fn handle_fanart_extension_request(
    req: media_extension_sdk::ExtensionRequest,
) -> media_extension_sdk::ExtensionResponse {
    let plugin = FanartTvPlugin::new();
    let config = ProviderConfig::from_extension_input(&req.input);

    let result_json = match req.operation.as_str() {
        "manifest" => serde_json::to_value(plugin.manifest()).unwrap_or_default(),
        "metadata.search" | "search" => {
            let Ok(query) = serde_json::from_value::<SearchQuery>(req.input) else {
                return media_extension_sdk::ExtensionResponse {
                    api_version: 1,
                    result: serde_json::json!({"error":"invalid_search_input"}),
                };
            };
            match block_on_guest(plugin.search(query, &config)) {
                Ok(items) => serde_json::to_value(items).unwrap_or_default(),
                Err(error) => serde_json::json!({"error":error.code}),
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
        _ => serde_json::json!({"error": "unsupported_operation"}),
    };

    media_extension_sdk::ExtensionResponse {
        api_version: 1,
        result: result_json,
    }
}
