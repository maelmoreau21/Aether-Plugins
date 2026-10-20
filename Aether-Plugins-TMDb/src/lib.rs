//! TMDb (The Movie Database) metadata provider extension for Aether.

use aether_plugin_api::{
    ActorCredit, ConfigField, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, ProviderConfig,
    ProviderRating, SearchQuery, SearchResultItem, StructuredMetadata,
};
use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use serde::Deserialize;
use std::collections::BTreeMap;
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
        if let Ok(key) = std::env::var("TMDB_API_KEY") {
            if !key.trim().is_empty() {
                // Leaked to static lifetime for return value convenience in process
                return Ok(Box::leak(key.into_boxed_str()));
            }
        }
        Err(PluginError::missing_api_key("tmdb"))
    }

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
}

#[async_trait]
impl MetadataProviderPlugin for TmdbPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: aether_plugin_api::API_VERSION,
            id: "tmdb".into(),
            name: "TMDb (The Movie Database)".into(),
            version: "0.1.0".into(),
            category: "Metadata".into(),
            provider: "tmdb".into(),
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
                    label: "Langue des métadonnées".into(),
                    field_type: "select".into(),
                    description: "Code langue ISO 639-1 / BCP 47 (ex. fr-FR, en-US)".into(),
                    required: false,
                    secret: false,
                    default_value: Some("fr-FR".into()),
                    options: vec!["fr-FR".into(), "en-US".into(), "es-ES".into(), "de-DE".into(), "ja-JP".into()],
                },
                ConfigField {
                    key: "include_adult".into(),
                    label: "Inclure le contenu adulte".into(),
                    field_type: "boolean".into(),
                    description: "true pour inclure les médias adultes dans les recherches".into(),
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
        let api_key = self.resolve_api_key(config)?;
        let (client, base_url, is_bearer) = self.build_client(config, api_key)?;
        let language = query
            .language
            .as_deref()
            .or(config.language.as_deref())
            .unwrap_or("fr-FR");

        let is_tv = matches!(query.media_type, MediaType::TvShow | MediaType::Episode);
        let endpoint = if is_tv {
            format!("{base_url}/search/tv")
        } else {
            format!("{base_url}/search/movie")
        };

        let mut req = client
            .get(&endpoint)
            .query(&[("query", query.title.as_str()), ("language", language)]);

        if !is_bearer {
            req = req.query(&[("api_key", api_key)]);
        }

        if let Some(year) = query.year {
            let year_str = year.to_string();
            if is_tv {
                req = req.query(&[("first_air_date_year", &year_str)]);
            } else {
                req = req.query(&[("year", &year_str)]);
            }
        }

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

        let data: TmdbSearchResponse = res
            .json()
            .await
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;

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
        let api_key = self.resolve_api_key(config)?;
        let (client, base_url, is_bearer) = self.build_client(config, api_key)?;
        let language = config.language.as_deref().unwrap_or("fr-FR");

        let is_tv = matches!(request.media_type, MediaType::TvShow | MediaType::Episode);
        let endpoint = if is_tv {
            format!("{base_url}/tv/{}", request.id)
        } else {
            format!("{base_url}/movie/{}", request.id)
        };

        let mut req = client.get(&endpoint).query(&[
            ("language", language),
            ("append_to_response", "credits,images,external_ids"),
            ("include_image_language", &format!("{language},en,null")),
        ]);

        if !is_bearer {
            req = req.query(&[("api_key", api_key)]);
        }

        let res = req
            .send()
            .await
            .map_err(|e| PluginError::new("network_error", e.to_string()))?;

        if res.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(PluginError::not_found(&request.id));
        }
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

        let d: TmdbDetailsResponse = res
            .json()
            .await
            .map_err(|e| PluginError::new("parse_error", e.to_string()))?;

        let title = d.title.or(d.name);
        let original_title = d.original_title.or(d.original_name);
        let release_date = d.release_date.or(d.first_air_date);
        let year = release_date
            .as_deref()
            .and_then(|s| s.split('-').next())
            .and_then(|y| y.parse::<u16>().ok());

        let genres = d.genres.into_iter().map(|g| g.name).collect();

        let mut directors = Vec::new();
        let mut writers = Vec::new();
        let mut actors = Vec::new();

        if let Some(credits) = d.credits {
            for crew in credits.crew {
                if crew.job == "Director" && !directors.contains(&crew.name) {
                    directors.push(crew.name);
                } else if (crew.department == "Writing" || crew.job == "Screenplay")
                    && !writers.contains(&crew.name)
                {
                    writers.push(crew.name);
                }
            }

            for cast in credits.cast.into_iter().take(30) {
                actors.push(ActorCredit {
                    name: cast.name,
                    role: cast.character.filter(|c| !c.is_empty()),
                    thumb_url: cast.profile_path.map(|p| format!("{IMAGE_PROFILE_URL}{p}")),
                    order: Some(cast.order),
                });
            }
        }

        let mut ratings = Vec::new();
        if let Some(avg) = d.vote_average {
            if avg > 0.0 {
                ratings.push(ProviderRating {
                    provider: "tmdb".into(),
                    value: avg,
                    max: 10.0,
                    votes: d.vote_count,
                });
            }
        }

        let mut external_ids = BTreeMap::new();
        external_ids.insert("tmdb".into(), request.id.clone());
        if let Some(ext) = d.external_ids {
            if let Some(imdb) = ext.imdb_id.filter(|s| !s.is_empty()) {
                external_ids.insert("imdb".into(), imdb);
            }
            if let Some(tvdb) = ext.tvdb_id {
                external_ids.insert("tvdb".into(), tvdb.to_string());
            }
        }

        let mut posters = Vec::new();
        let mut backdrops = Vec::new();
        let mut all_images = Vec::new();

        if let Some(images) = d.images {
            for (idx, p) in images.posters.into_iter().enumerate() {
                let img = MediaImage {
                    url: format!("{IMAGE_ORIGINAL_URL}{}", p.file_path),
                    image_type: ImageType::Poster,
                    width: Some(p.width),
                    height: Some(p.height),
                    language: p.iso_639_1,
                    rating: Some(p.vote_average),
                    vote_count: Some(p.vote_count),
                    is_primary: idx == 0,
                };
                posters.push(img.clone());
                all_images.push(img);
            }

            for (idx, b) in images.backdrops.into_iter().enumerate() {
                let img = MediaImage {
                    url: format!("{IMAGE_ORIGINAL_URL}{}", b.file_path),
                    image_type: ImageType::Backdrop,
                    width: Some(b.width),
                    height: Some(b.height),
                    language: b.iso_639_1,
                    rating: Some(b.vote_average),
                    vote_count: Some(b.vote_count),
                    is_primary: idx == 0,
                };
                backdrops.push(img.clone());
                all_images.push(img);
            }

            for (idx, l) in images.logos.into_iter().enumerate() {
                let img = MediaImage {
                    url: format!("{IMAGE_ORIGINAL_URL}{}", l.file_path),
                    image_type: ImageType::Logo,
                    width: Some(l.width),
                    height: Some(l.height),
                    language: l.iso_639_1,
                    rating: Some(l.vote_average),
                    vote_count: Some(l.vote_count),
                    is_primary: idx == 0,
                };
                all_images.push(img);
            }
        }

        // Fallback for primary poster and backdrop from main details if images list was empty
        if posters.is_empty() {
            if let Some(path) = d.poster_path {
                let img = MediaImage {
                    url: format!("{IMAGE_ORIGINAL_URL}{path}"),
                    image_type: ImageType::Poster,
                    width: None,
                    height: None,
                    language: None,
                    rating: None,
                    vote_count: None,
                    is_primary: true,
                };
                posters.push(img.clone());
                all_images.push(img);
            }
        }

        if backdrops.is_empty() {
            if let Some(path) = d.backdrop_path {
                let img = MediaImage {
                    url: format!("{IMAGE_ORIGINAL_URL}{path}"),
                    image_type: ImageType::Backdrop,
                    width: None,
                    height: None,
                    language: None,
                    rating: None,
                    vote_count: None,
                    is_primary: true,
                };
                backdrops.push(img.clone());
                all_images.push(img);
            }
        }

        Ok(StructuredMetadata {
            provider: "tmdb".into(),
            provider_id: request.id,
            external_ids,
            title,
            original_title,
            tagline: d.tagline.filter(|s| !s.trim().is_empty()),
            synopsis: d.overview.filter(|s| !s.trim().is_empty()),
            year,
            release_date,
            runtime_minutes: d
                .runtime
                .or_else(|| d.episode_run_time.and_then(|v| v.first().copied())),
            genres,
            directors,
            writers,
            actors,
            ratings,
            mpaa: None,
            status: d.status,
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

// TMDb JSON models
#[derive(Deserialize)]
struct TmdbSearchResponse {
    #[serde(default)]
    results: Vec<TmdbSearchResult>,
}

#[derive(Deserialize)]
struct TmdbSearchResult {
    id: u64,
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

#[derive(Deserialize)]
struct TmdbDetailsResponse {
    title: Option<String>,
    name: Option<String>,
    original_title: Option<String>,
    original_name: Option<String>,
    tagline: Option<String>,
    overview: Option<String>,
    release_date: Option<String>,
    first_air_date: Option<String>,
    runtime: Option<u32>,
    episode_run_time: Option<Vec<u32>>,
    status: Option<String>,
    poster_path: Option<String>,
    backdrop_path: Option<String>,
    vote_average: Option<f64>,
    vote_count: Option<u64>,
    #[serde(default)]
    genres: Vec<TmdbGenre>,
    credits: Option<TmdbCredits>,
    images: Option<TmdbImages>,
    external_ids: Option<TmdbExternalIds>,
}

#[derive(Deserialize)]
struct TmdbGenre {
    name: String,
}

#[derive(Deserialize)]
struct TmdbCredits {
    #[serde(default)]
    cast: Vec<TmdbCast>,
    #[serde(default)]
    crew: Vec<TmdbCrew>,
}

#[derive(Deserialize)]
struct TmdbCast {
    name: String,
    character: Option<String>,
    profile_path: Option<String>,
    order: u32,
}

#[derive(Deserialize)]
struct TmdbCrew {
    name: String,
    job: String,
    department: String,
}

#[derive(Deserialize)]
struct TmdbImages {
    #[serde(default)]
    posters: Vec<TmdbImageFile>,
    #[serde(default)]
    backdrops: Vec<TmdbImageFile>,
    #[serde(default)]
    logos: Vec<TmdbImageFile>,
}

#[derive(Deserialize)]
struct TmdbImageFile {
    file_path: String,
    width: u32,
    height: u32,
    iso_639_1: Option<String>,
    #[serde(default)]
    vote_average: f64,
    #[serde(default)]
    vote_count: u64,
}

#[derive(Deserialize)]
struct TmdbExternalIds {
    imdb_id: Option<String>,
    tvdb_id: Option<u64>,
}
