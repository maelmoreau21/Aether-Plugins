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
// The SDK owns the URL encoder and the synchronous guest executor (identical behaviour).
use aether_plugin_api::http::{block_on_guest, urlencode};

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

struct DetailsExtras {
    original_language: Option<String>,
    actor_ids: Vec<Option<String>>,
    crew_credits: Vec<serde_json::Value>,
}

fn person_id(credit: &serde_json::Value) -> Option<String> {
    credit
        .get("id")
        .and_then(|id| id.as_u64())
        .filter(|id| *id > 0)
        .map(|id| format!("tmdb:{id}"))
}

fn profile_url(credit: &serde_json::Value) -> Option<String> {
    credit
        .get("profile_path")
        .and_then(|path| path.as_str())
        .filter(|path| {
            path.starts_with('/')
                && !path.starts_with("//")
                && path.len() <= 512
                && !path.chars().any(char::is_control)
        })
        .map(|path| format!("{IMAGE_PROFILE_URL}{path}"))
}

impl TmdbPlugin {
    pub fn new() -> Self {
        Self
    }

    async fn resolve_id(
        &self,
        id: &str,
        external_ids: &BTreeMap<String, String>,
        media_type: MediaType,
        config: &ProviderConfig,
    ) -> Result<(String, bool), PluginError> {
        let numeric = |s: &str| {
            !s.is_empty()
                && s.bytes().all(|b| b.is_ascii_digit())
                && s.parse::<u64>().is_ok_and(|n| n > 0)
        };
        if let Some(tmdb) = external_ids
            .get("tmdb")
            .map(|s| s.trim())
            .filter(|s| numeric(s))
        {
            return Ok((tmdb.to_owned(), false));
        }
        let id = id.trim();
        if numeric(id) {
            return Ok((id.to_owned(), false));
        }
        // A blank entry is no identifier: it must not shadow a usable one behind it.
        let present = |key: &str| {
            external_ids
                .get(key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
        };
        let external = present("imdb")
            .map(|s| (s, "imdb_id"))
            .or_else(|| id.starts_with("tt").then_some((id, "imdb_id")))
            .or_else(|| present("tvdb").map(|s| (s, "tvdb_id")));
        let Some((external, source)) = external else {
            return Err(PluginError::new(
                "invalid_tmdb_id",
                "A valid TMDb, IMDb or TVDB identifier is required",
            ));
        };
        if (source == "imdb_id" && (!external.starts_with("tt") || !numeric(&external[2..])))
            || (source == "tvdb_id" && !numeric(external))
        {
            return Err(PluginError::new(
                "invalid_external_id",
                "Invalid external identifier",
            ));
        }
        let found: serde_json::Value = self
            .fetch_json(
                &format!("find/{external}"),
                &[("external_source", source)],
                config,
            )
            .await?;
        let key = if matches!(media_type, MediaType::TvShow | MediaType::Episode) {
            "tv_results"
        } else {
            "movie_results"
        };
        // An external id names one work. Several distinct matches are reported, never
        // resolved by picking the first: the Core decides what to do with an ambiguity.
        let mut matches = found
            .get(key)
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter_map(|v| v.get("id").and_then(|v| v.as_u64()))
            .filter(|id| *id > 0);
        let Some(first) = matches.next() else {
            return Err(PluginError::not_found(external));
        };
        if matches.any(|id| id != first) {
            return Err(PluginError::new(
                "ambiguous_match",
                "The external identifier matches several TMDb entries",
            ));
        }
        Ok((first.to_string(), true))
    }

    fn resolve_api_key<'a>(
        &self,
        config: &'a ProviderConfig,
    ) -> Result<std::borrow::Cow<'a, str>, PluginError> {
        if let Some(key) = config.api_key.as_deref().filter(|s| !s.trim().is_empty()) {
            return Ok(std::borrow::Cow::Borrowed(key.trim()));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(key) = std::env::var("TMDB_API_KEY")
            && !key.trim().is_empty()
        {
            return Ok(std::borrow::Cow::Owned(key.trim().to_string()));
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
        if is_bearer && let Ok(val) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
            headers.insert(AUTHORIZATION, val);
        }

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .default_headers(headers)
            // The Core broker refuses redirects; following one natively would resend the
            // bearer token to whatever host the answer names.
            .redirect(reqwest::redirect::Policy::none())
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
            query_params.push(("api_key", api_key.as_ref()));
        }

        let query_string = query_params
            .iter()
            .map(|(k, v)| format!("{k}={}", urlencode(v)))
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

            // One request, no retry: the Core turns 429/503 into a durable cooldown. The
            // request URL carries the v3 key, so neither error path may echo it.
            let resp = media_extension_sdk::host_http_fetch(&host_req)
                .map_err(aether_plugin_api::http::host_error)?;
            if let Some(error) = aether_plugin_api::http::classify_status(
                resp.status,
                aether_plugin_api::http::retry_after_from_headers(&resp.headers),
                "TMDb",
                endpoint,
            ) {
                return Err(error);
            }
            serde_json::from_slice(&resp.body).map_err(|e| aether_plugin_api::http::json_error(&e))
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let (client, _, _) = self.build_client(config, api_key.as_ref())?;
            let res = client
                .get(&full_url)
                .send()
                .await
                .map_err(aether_plugin_api::http::transport_error)?;
            let retry_after = aether_plugin_api::http::retry_after_secs(
                res.headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok()),
            );
            if let Some(error) = aether_plugin_api::http::classify_status(
                res.status().as_u16(),
                retry_after,
                "TMDb",
                endpoint,
            ) {
                return Err(error);
            }
            let body = aether_plugin_api::http::read_body(res).await?;
            serde_json::from_slice(&body).map_err(|e| aether_plugin_api::http::json_error(&e))
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
            version: env!("CARGO_PKG_VERSION").into(),
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
                    key: "timeout_secs".into(), label: "Délai de réponse (secondes)".into(),
                    description: "Durée maximale de chaque requête au fournisseur".into(),
                    field_type: "number".into(), required: false, secret: false,
                    default_value: Some("10".into()), options: vec![],
                },
                ConfigField {
                    key: "language".into(), label: "Langue des métadonnées".into(),
                    description: "Code de langue des titres et descriptions (par exemple fr-FR)".into(),
                    field_type: "text".into(), required: false, secret: false,
                    default_value: Some("fr-FR".into()), options: vec![],
                },
                ConfigField {
                    key: "base_url".into(),
                    label: "Adresse du fournisseur".into(),
                    description: "Adresse officielle du fournisseur, modifiable si nécessaire".into(),
                    field_type: "text".into(),
                    required: false,
                    secret: false,
                    default_value: Some(DEFAULT_BASE_URL.into()),
                    options: vec![],
                },
                ConfigField {
                    key: "api_key".into(),
                    label: "Clé API TMDb".into(),
                    field_type: "password".into(),
                    description: "Clé API TMDb v3 ou jeton d'accès v4 Bearer ; le serveur peut fournir une clé par défaut".into(),
                    required: false,
                    secret: true,
                    default_value: None,
                    options: vec![],
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
        // Jellyfin resolves known provider IDs before attempting any title search.
        // A localized NFO title must never replace an authoritative identifier.
        if ["tmdb", "imdb", "tvdb"].iter().any(|key| {
            query
                .external_ids
                .get(*key)
                .is_some_and(|v| !v.trim().is_empty())
        }) {
            let meta = self
                .details(
                    DetailsRequest {
                        id: String::new(),
                        media_type: query.media_type,
                        external_ids: query.external_ids.clone(),
                        ..Default::default()
                    },
                    config,
                )
                .await?;
            return Ok(vec![SearchResultItem {
                id: meta.provider_id,
                title: meta.title.unwrap_or_else(|| query.title.clone()),
                artist: None,
                original_title: meta.original_title,
                year: meta.year,
                media_type: query.media_type,
                overview: meta.synopsis,
                poster_url: meta.posters.first().map(|image| image.url.clone()),
                score: meta.ratings.first().map(|rating| rating.value),
            }]);
        }
        let language = query
            .language
            .as_deref()
            .or(config.language.as_deref())
            .unwrap_or("fr-FR");

        let is_tv = matches!(query.media_type, MediaType::TvShow | MediaType::Episode);
        let endpoint = if is_tv { "search/tv" } else { "search/movie" };

        let adult = if config
            .extra
            .get("include_adult")
            .is_some_and(|v| v == "true")
        {
            "true"
        } else {
            "false"
        };
        let mut params = vec![
            ("query", query.title.as_str()),
            ("language", language),
            ("include_adult", adult),
        ];

        let year_str = query.year.map(|y| y.to_string());
        if let Some(y) = &year_str {
            if is_tv {
                params.push(("first_air_date_year", y.as_str()));
            } else {
                params.push(("year", y.as_str()));
            }
        }

        let mut data: TmdbSearchResponse = self.fetch_json(endpoint, &params, config).await?;

        // A wrong or release-year-offset filter must not hide the right title: retry without it.
        if data.results.is_empty() && year_str.is_some() {
            let relaxed = [
                ("query", query.title.as_str()),
                ("language", language),
                ("include_adult", adult),
            ];
            data = self.fetch_json(endpoint, &relaxed, config).await?;
        }

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
                    artist: None,
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
        Ok(self.details_with_language(request, config).await?.0)
    }

    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        let is_tv = matches!(request.media_type, MediaType::TvShow | MediaType::Episode);
        let (id, _) = self
            .resolve_id(
                &request.id,
                &request.external_ids,
                request.media_type,
                config,
            )
            .await?;
        let endpoint = if is_tv {
            format!("tv/{id}/images")
        } else {
            format!("movie/{id}/images")
        };

        // Without a language policy the whole unfiltered list is kept (historical
        // behaviour). With one, the provider is asked for exactly those languages plus
        // textless art, so the response and the host's 64 KiB envelope stay small. Images
        // carry a language only (`fr`), never a region.
        let filter = image_language_filter(config);
        let params: Vec<(&str, &str)> = filter
            .as_deref()
            .map(|languages| vec![("include_image_language", languages)])
            .unwrap_or_default();
        let val: serde_json::Value = self.fetch_json(&endpoint, &params, config).await?;
        let mut images = Vec::new();
        for (key, image_type) in [
            ("posters", ImageType::Poster),
            ("backdrops", ImageType::Backdrop),
            ("logos", ImageType::Logo),
        ] {
            if let Some(items) = val.get(key).and_then(|v| v.as_array()) {
                for (idx, item) in items.iter().enumerate() {
                    if let Some(path) = item
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .filter(|path| path.starts_with('/') && !path.starts_with("//"))
                    {
                        images.push(MediaImage {
                            url: format!("{IMAGE_ORIGINAL_URL}{path}"),
                            image_type,
                            width: item
                                .get("width")
                                .and_then(|v| v.as_u64())
                                .and_then(|v| u32::try_from(v).ok()),
                            height: item
                                .get("height")
                                .and_then(|v| v.as_u64())
                                .and_then(|v| u32::try_from(v).ok()),
                            language: item
                                .get("iso_639_1")
                                .and_then(|v| v.as_str())
                                .filter(|v| !v.is_empty())
                                .map(str::to_owned),
                            rating: item.get("vote_average").and_then(|v| v.as_f64()),
                            vote_count: item.get("vote_count").and_then(|v| v.as_u64()),
                            is_primary: idx == 0,
                        });
                    }
                }
            }
        }
        Ok(config.select_images(images))
    }
}

/// `include_image_language` value for the configured language policy, or `None` to keep
/// every language. The provider only knows two-letter languages and the `null` bucket.
fn image_language_filter(config: &ProviderConfig) -> Option<String> {
    let mut languages: Vec<String> = Vec::new();
    for tag in config.image_languages() {
        if tag.len() == 2 && !languages.contains(&tag) {
            languages.push(tag);
        }
    }
    if languages.is_empty() {
        return None;
    }
    if config.include_neutral.unwrap_or(true) {
        languages.push("null".into());
    }
    Some(languages.join(","))
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

impl TmdbPlugin {
    /// Wire form of `details`: the structured metadata plus the `originalLanguage`
    /// field, which `StructuredMetadata` does not carry. Built by serializing the
    /// struct (no `Deserialize` round-trip, which would bloat the Wasm module).
    pub async fn details_value(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<serde_json::Value, PluginError> {
        let (metadata, extras) = self.details_with_language(request, config).await?;
        let mut result = serde_json::to_value(metadata).unwrap_or_default();
        if let Some(language) = extras.original_language {
            result["originalLanguage"] = serde_json::Value::String(language);
        }
        if let Some(actors) = result
            .get_mut("actors")
            .and_then(|value| value.as_array_mut())
        {
            for (actor, id) in actors.iter_mut().zip(extras.actor_ids) {
                if let Some(id) = id {
                    actor["providerId"] = serde_json::Value::String(id);
                }
            }
        }
        if !extras.crew_credits.is_empty() {
            result["crewCredits"] = serde_json::Value::Array(extras.crew_credits);
        }
        Ok(result)
    }

    async fn details_with_language(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<(StructuredMetadata, DetailsExtras), PluginError> {
        let language = config.language.as_deref().unwrap_or("fr-FR");
        let main_locale = aether_plugin_api::canonical_locale(language);
        let is_tv = matches!(request.media_type, MediaType::TvShow | MediaType::Episode);
        // Each Core invocation may issue at most `MAX_REQUESTS` calls, paced one second
        // apart; every optional call below is spent from this budget, never assumed free.
        let mut used = 0usize;
        let (id, looked_up) = self
            .resolve_id(
                &request.id,
                &request.external_ids,
                request.media_type,
                config,
            )
            .await?;
        used += usize::from(looked_up);
        let endpoint = if is_tv {
            format!("tv/{id}")
        } else {
            format!("movie/{id}")
        };

        // `credits` and `external_ids` are the only appended parts the code reads. Images
        // stay in the dedicated images operation (own budget, language-filtered) and
        // translations are one explicit call below: appending either to this response risks
        // the 256 KiB cap on large casts.
        let params = [
            ("language", language),
            ("append_to_response", "credits,external_ids"),
        ];

        used += 1;
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
        let mut actor_ids = Vec::new();
        let mut crew_credits = Vec::new();

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
                        if matches!(job, "Director" | "Screenplay" | "Writer")
                            && !n.trim().is_empty()
                            && n.len() <= 512
                            && crew_credits.len() < 100
                        {
                            crew_credits.push(serde_json::json!({"name": n, "role": job, "thumbUrl": profile_url(c), "providerId": person_id(c)}));
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
                        let thumb_url = profile_url(c);
                        actors.push(ActorCredit {
                            name: name.to_string(),
                            role,
                            thumb_url,
                            order: Some(idx as u32),
                        });
                        actor_ids.push(person_id(c));
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
        external_ids.insert("tmdb".into(), id.clone());
        if let Some(ext) = val.get("external_ids") {
            if let Some(imdb) = ext.get("imdb_id").and_then(|v| v.as_str()) {
                external_ids.insert("imdb".into(), imdb.to_string());
            }
            if let Some(tvdb) = ext.get("tvdb_id").and_then(|v| v.as_u64()) {
                external_ids.insert("tvdb".into(), tvdb.to_string());
            }
        }

        // `poster_path` / `backdrop_path` are the provider's pick for the requested language
        // (then the original language, then the best rated): the language of the file itself
        // is not reported, so it is left unknown instead of being labelled with the request.
        let mut posters = Vec::new();
        let mut backdrops = Vec::new();
        if let Some(poster_path) = val.get("poster_path").and_then(|v| v.as_str()) {
            posters.push(MediaImage {
                url: format!("{IMAGE_ORIGINAL_URL}{poster_path}"),
                image_type: ImageType::Poster,
                width: None,
                height: None,
                language: None,
                rating: None,
                vote_count: None,
                is_primary: true,
            });
        }
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

        // Other languages: one request returns every language the provider has, regional
        // variants included (`fr-FR` and `fr-CA` are separate entries).
        let wanted = wanted_locales(&request.languages, main_locale.as_deref());
        let mut translations = BTreeMap::new();
        if !wanted.is_empty() {
            used += 1;
            let url = format!("{endpoint}/translations");
            match self
                .fetch_json::<serde_json::Value>(&url, &[], config)
                .await
            {
                Ok(value) => translations = parse_translations(&value, &wanted),
                Err(e) if is_fatal(&e) => return Err(e),
                // A missing translation list must not discard the main-language details.
                Err(_) => {}
            }
        }

        let mut seasons = Vec::new();
        if is_tv && let Some(s_arr) = val.get("seasons").and_then(|v| v.as_array()) {
            // Real seasons first, in order; specials (season 0) last, so a short budget
            // never spends its calls on extras.
            let mut order: Vec<&serde_json::Value> = s_arr.iter().collect();
            order.sort_by_key(|s| {
                let n = s.get("season_number").and_then(|v| v.as_u64()).unwrap_or(0);
                (n == 0, n)
            });
            let mut fetched: Vec<u32> = Vec::new();
            let mut by_number: BTreeMap<u32, aether_plugin_api::SeasonMetadata> = BTreeMap::new();
            let mut exhausted = false;
            for s in order {
                let s_num = s
                    .get("season_number")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as u32)
                    .unwrap_or(0);
                let mut season = aether_plugin_api::SeasonMetadata {
                    season_number: s_num,
                    title: s.get("name").and_then(|v| v.as_str()).map(str::to_owned),
                    overview: s
                        .get("overview")
                        .and_then(|v| v.as_str())
                        .filter(|s| !s.trim().is_empty())
                        .map(str::to_owned),
                    poster_url: s
                        .get("poster_path")
                        .and_then(|v| v.as_str())
                        .map(|p| format!("{IMAGE_ORIGINAL_URL}{p}")),
                    ..Default::default()
                };
                if !exhausted && used < MAX_REQUESTS {
                    used += 1;
                    let endpoint = format!("tv/{id}/season/{s_num}");
                    match self
                        .fetch_json::<serde_json::Value>(
                            &endpoint,
                            &[("language", language)],
                            config,
                        )
                        .await
                    {
                        Ok(season_val) => {
                            season.episodes = parse_episodes(&season_val);
                            fetched.push(s_num);
                        }
                        Err(e) if is_fatal(&e) => return Err(e),
                        Err(e) if e.code == "quota_exceeded" => exhausted = true,
                        Err(_) => {}
                    }
                }
                by_number.insert(s_num, season);
            }
            // Spare budget buys the same seasons in the other requested languages, in
            // season order. Coverage may be partial on long series; the reader's fallback
            // chain fills what is missing, field by field.
            'extra: for extra in wanted
                .iter()
                .filter(|l| Some(l.as_str()) != main_locale.as_deref())
                .take(2)
            {
                for number in &fetched {
                    if used >= MAX_REQUESTS || exhausted {
                        break 'extra;
                    }
                    used += 1;
                    let endpoint = format!("tv/{id}/season/{number}");
                    match self
                        .fetch_json::<serde_json::Value>(
                            &endpoint,
                            &[("language", extra.as_str())],
                            config,
                        )
                        .await
                    {
                        Ok(season_val) => {
                            if let Some(season) = by_number.get_mut(number) {
                                merge_season_language(season, &season_val, extra);
                            }
                        }
                        Err(e) if is_fatal(&e) => return Err(e),
                        Err(e) if e.code == "quota_exceeded" => exhausted = true,
                        Err(_) => {}
                    }
                }
            }
            seasons = by_number.into_values().collect();
        }

        let collection = val.get("belongs_to_collection").and_then(|collection| {
            let id = collection.get("id")?.as_u64()?;
            let name = collection.get("name")?.as_str()?.trim();
            (!name.is_empty() && name.len() <= 300).then(|| aether_plugin_api::MetadataCollection {
                id: format!("tmdb:{id}"),
                name: name.to_owned(),
            })
        });
        let original_language = val
            .get("original_language")
            .and_then(|v| v.as_str())
            .filter(|v| {
                !v.is_empty()
                    && v.len() <= 35
                    && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
            .map(str::to_owned);
        let mut metadata = StructuredMetadata {
            collection,
            provider: "tmdb".into(),
            provider_id: id,
            external_ids,
            title,
            original_title,
            original_language: original_language.clone(),
            locale: main_locale,
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
            images: Vec::new(),
            seasons,
            translations,
        };
        let reserved = serde_json::to_vec(&crew_credits).map_or(0, |v| v.len()) + 2048;
        fit_output(&mut metadata, reserved);
        Ok((
            metadata,
            DetailsExtras {
                original_language,
                actor_ids,
                crew_credits,
            },
        ))
    }
}

/// Host quota of one Core invocation (`DEFAULT_MAX_HTTP_REQUESTS`).
const MAX_REQUESTS: usize = 10;
/// The extension response is capped at 64 KiB by the ABI; stay below with room for the envelope.
const MAX_OUTPUT_BYTES: usize = 60 * 1024;
const MAX_TRANSLATIONS: usize = 16;

/// Errors the whole details call must report instead of returning half a record.
fn is_fatal(error: &PluginError) -> bool {
    matches!(
        error.code.as_str(),
        "unauthorized" | "forbidden" | "rate_limited" | "missing_api_key"
    )
}

/// Extra languages the Core asked for, canonical and without duplicates, plus the main one
/// so its regional entry is kept too. Empty when the Core asked for nothing else.
fn wanted_locales(requested: &[String], main: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in requested.iter().take(8) {
        if let Some(tag) = aether_plugin_api::canonical_locale(tag)
            && !out.contains(&tag)
        {
            out.push(tag);
        }
    }
    let extra = out.iter().any(|t| Some(t.as_str()) != main);
    if !extra {
        return Vec::new();
    }
    if let Some(main) = main
        && !out.iter().any(|t| t == main)
    {
        out.insert(0, main.to_owned());
    }
    out
}

fn non_empty(value: Option<&serde_json::Value>) -> Option<String> {
    value
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Keep the entries for the requested languages. Keys keep the region when the provider
/// gives one (`fr-CA`); a request for bare `fr` accepts every French entry.
fn parse_translations(
    value: &serde_json::Value,
    wanted: &[String],
) -> BTreeMap<String, aether_plugin_api::TranslationItem> {
    let bases: Vec<&str> = wanted
        .iter()
        .map(|t| t.split('-').next().unwrap_or(t))
        .collect();
    let mut out = BTreeMap::new();
    for item in value
        .get("translations")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
    {
        let Some(iso) = item
            .get("iso_639_1")
            .and_then(|v| v.as_str())
            .filter(|v| v.len() == 2)
        else {
            continue;
        };
        let region = item
            .get("iso_3166_1")
            .and_then(|v| v.as_str())
            .filter(|v| v.len() == 2);
        let key = match region {
            Some(region) => format!(
                "{}-{}",
                iso.to_ascii_lowercase(),
                region.to_ascii_uppercase()
            ),
            None => iso.to_ascii_lowercase(),
        };
        if !bases.contains(&key.split('-').next().unwrap_or("")) || out.len() >= MAX_TRANSLATIONS {
            continue;
        }
        let Some(data) = item.get("data") else {
            continue;
        };
        let entry = aether_plugin_api::TranslationItem {
            title: non_empty(data.get("title").or_else(|| data.get("name"))),
            overview: non_empty(data.get("overview")),
            tagline: non_empty(data.get("tagline")),
        };
        // TMDb returns empty strings for languages it has not translated.
        if entry.title.is_some() || entry.overview.is_some() || entry.tagline.is_some() {
            out.insert(key, entry);
        }
    }
    out
}

fn parse_episodes(season: &serde_json::Value) -> Vec<aether_plugin_api::EpisodeMetadata> {
    season
        .get("episodes")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .map(|ep| aether_plugin_api::EpisodeMetadata {
            episode_number: ep
                .get("episode_number")
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
                .unwrap_or(0),
            title: ep.get("name").and_then(|v| v.as_str()).map(str::to_owned),
            overview: non_empty(ep.get("overview")),
            air_date: ep
                .get("air_date")
                .and_then(|v| v.as_str())
                .map(str::to_owned),
            runtime_minutes: ep.get("runtime").and_then(|v| v.as_u64()).map(|r| r as u32),
            still_url: ep
                .get("still_path")
                .and_then(|v| v.as_str())
                .map(|p| format!("{IMAGE_ORIGINAL_URL}{p}")),
            rating: ep.get("vote_average").and_then(|v| v.as_f64()),
            translations: BTreeMap::new(),
        })
        .collect()
}

/// Attach another language's season and episode texts under `tag`.
fn merge_season_language(
    season: &mut aether_plugin_api::SeasonMetadata,
    response: &serde_json::Value,
    tag: &str,
) {
    let item = aether_plugin_api::TranslationItem {
        title: non_empty(response.get("name")),
        overview: non_empty(response.get("overview")),
        tagline: None,
    };
    if item.title.is_some() || item.overview.is_some() {
        season.translations.insert(tag.to_owned(), item);
    }
    for ep in parse_episodes(response) {
        let entry = aether_plugin_api::TranslationItem {
            title: ep.title.filter(|t| !t.trim().is_empty()),
            overview: ep.overview,
            tagline: None,
        };
        if entry.title.is_none() && entry.overview.is_none() {
            continue;
        }
        if let Some(target) = season
            .episodes
            .iter_mut()
            .find(|e| e.episode_number == ep.episode_number)
        {
            target.translations.insert(tag.to_owned(), entry);
        }
    }
}

/// Shrink the record until its JSON fits the ABI output cap, dropping the least valuable
/// data first: extra-language episode text, then long episode overviews, then the episodes
/// of the last seasons. Identifiers, titles and credits are never touched.
fn fit_output(metadata: &mut StructuredMetadata, reserved: usize) {
    let size =
        |m: &StructuredMetadata| serde_json::to_vec(m).map_or(usize::MAX, |v| v.len()) + reserved;
    if size(metadata) <= MAX_OUTPUT_BYTES {
        return;
    }
    for season in &mut metadata.seasons {
        season.translations.clear();
        for ep in &mut season.episodes {
            ep.translations.clear();
        }
    }
    if size(metadata) <= MAX_OUTPUT_BYTES {
        return;
    }
    for season in &mut metadata.seasons {
        for ep in &mut season.episodes {
            if let Some(overview) = ep.overview.as_mut() {
                *overview = overview.chars().take(200).collect();
            }
        }
    }
    while size(metadata) > MAX_OUTPUT_BYTES {
        match metadata
            .seasons
            .iter_mut()
            .rev()
            .find(|s| !s.episodes.is_empty())
        {
            Some(season) => season.episodes.clear(),
            None => break,
        }
    }
    if size(metadata) > MAX_OUTPUT_BYTES {
        for item in metadata.translations.values_mut() {
            item.overview = item
                .overview
                .as_ref()
                .map(|o| o.chars().take(500).collect());
        }
    }
}

media_extension_sdk::extension!(handle_tmdb_extension_request);

pub fn handle_tmdb_extension_request(
    req: media_extension_sdk::ExtensionRequest,
) -> media_extension_sdk::ExtensionResponse {
    if req.api_version != 1 {
        return media_extension_sdk::ExtensionResponse {
            api_version: 1,
            result: serde_json::json!({"error": "unsupported_api_version"}),
        };
    }
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
            match block_on_guest(plugin.details_value(d, &config)) {
                Ok(meta) => meta,
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
