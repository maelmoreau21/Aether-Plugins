//! fanart.tv high-quality artwork and image provider extension for Aether.

use aether_plugin_api::{
    ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, ProviderConfig,
    SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::BTreeMap;
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
            id: "fanart-tv".into(),
            name: "fanart.tv".into(),
            version: "0.1.0".into(),
            provider: "fanart.tv".into(),
            description: "Fournisseur d'images haute définition (ClearLogos, Backdrops 4K/HD, ClearArt, Bannières, Disques)".into(),
            author: "Aether Team".into(),
            license: "MIT OR Apache-2.0".into(),
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
                    description: "Clé API fanart.tv pour votre application (obtenue sur fanart.tv)".into(),
                    required: true,
                    secret: true,
                    default_value: None,
                },
                ConfigField {
                    key: "user_key".into(),
                    label: "Clé Utilisateur VIP (optionnelle)".into(),
                    description: "Clé personnelle VIP pour un accès prioritaire aux dernières contributions".into(),
                    required: false,
                    secret: true,
                    default_value: None,
                },
                ConfigField {
                    key: "base_url".into(),
                    label: "URL de base personnalisée".into(),
                    description: "URL alternative pour proxy ou serveur de test mock".into(),
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
        _config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        // fanart.tv is an artwork provider, indexed by external ID (TMDb, IMDb, TVDB)
        if let Some(tmdb_id) = query.external_ids.get("tmdb") {
            return Ok(vec![SearchResultItem {
                id: tmdb_id.clone(),
                title: query.title,
                original_title: None,
                year: query.year,
                media_type: query.media_type,
                overview: Some("Indexed via TMDb ID on fanart.tv".into()),
                poster_url: None,
                score: None,
            }]);
        }
        if let Some(imdb_id) = query.external_ids.get("imdb") {
            return Ok(vec![SearchResultItem {
                id: imdb_id.clone(),
                title: query.title,
                original_title: None,
                year: query.year,
                media_type: query.media_type,
                overview: Some("Indexed via IMDb ID on fanart.tv".into()),
                poster_url: None,
                score: None,
            }]);
        }
        // Direct title search is not supported by fanart.tv API
        Ok(Vec::new())
    }

    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        let api_key = self.resolve_api_key(config)?;
        let (client, base_url) = self.build_client(config)?;

        let is_tv = matches!(request.media_type, MediaType::TvShow | MediaType::Episode);
        let id = if !request.id.is_empty() {
            request.id.clone()
        } else if let Some(tmdb) = request.external_ids.get("tmdb") {
            tmdb.clone()
        } else if let Some(imdb) = request.external_ids.get("imdb") {
            imdb.clone()
        } else if let Some(tvdb) = request.external_ids.get("tvdb") {
            tvdb.clone()
        } else {
            return Err(PluginError::new(
                "missing_id",
                "fanart.tv requires TMDb, IMDb or TVDB ID",
            ));
        };

        let endpoint = if is_tv {
            format!("{base_url}/tv/{id}")
        } else {
            format!("{base_url}/movies/{id}")
        };

        let mut req = client.get(&endpoint).query(&[("api_key", api_key)]);
        if let Some(user_key) = config.user_key.as_deref().filter(|s| !s.trim().is_empty()) {
            req = req.header("user-key", user_key);
        }

        let res = req
            .send()
            .await
            .map_err(|e| PluginError::new("network_error", e.to_string()))?;

        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(PluginError::not_found(&id));
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

        let mut posters = Vec::new();
        let mut backdrops = Vec::new();
        let mut all_images = Vec::new();
        let mut external_ids = BTreeMap::new();
        let title: Option<String>;

        if is_tv {
            let data: FanartTvShowResponse = res
                .json()
                .await
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
            let data: FanartMovieResponse = res
                .json()
                .await
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
        Ok(details.images)
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
