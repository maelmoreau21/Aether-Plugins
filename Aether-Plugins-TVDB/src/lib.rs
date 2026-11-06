//! Optional TheTVDB v4 adapter; no key, licence or shared subscriber PIN bundled.
use aether_plugin_api::{
    ActorCredit, ConfigField, ConfigOption, DetailsRequest, ExtensionRequest, ExtensionResponse,
    ImageType, ImagesRequest, MediaImage, MediaType, MetadataProviderPlugin, PluginCapability,
    PluginError, PluginManifest, ProviderConfig, SearchQuery, SearchResultItem, StructuredMetadata,
    TranslationItem,
};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const API: &str = "https://api4.thetvdb.com/v4";
const MAX_RESPONSE: usize = 262_144;

#[derive(Default)]
pub struct TvdbPlugin;
fn failure(code: &str) -> PluginError {
    PluginError::new(code, "TheTVDB request could not be completed")
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 16
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|v| v > 0)
}
fn kind(media: MediaType) -> Result<&'static str, PluginError> {
    match media {
        MediaType::Movie => Ok("movies"),
        MediaType::TvShow | MediaType::Anime => Ok("series"),
        MediaType::Episode => Ok("episodes"),
        _ => Err(failure("unsupported_media_type")),
    }
}
fn bounded(value: Option<&str>, max: usize) -> Option<String> {
    value
        .map(|v| {
            v.chars()
                .filter(|c| !c.is_control() || *c == '\n')
                .take(max)
                .collect::<String>()
        })
        .filter(|v| !v.trim().is_empty())
}
fn text(value: &Value, key: &str, max: usize) -> Option<String> {
    bounded(value[key].as_str(), max)
}
fn code(language: &str) -> Result<String, PluginError> {
    let tag = language.trim().to_ascii_lowercase();
    let base = tag.split(['-', '_']).next().unwrap_or("");
    let mapped = match base {
        "fr" | "fre" => "fra",
        "en" => "eng",
        "de" | "ger" => "deu",
        "es" => "spa",
        "it" => "ita",
        "pt" => "por",
        "ja" => "jpn",
        "zh" | "chi" => "zho",
        "ko" => "kor",
        "ru" => "rus",
        "nl" | "dut" => "nld",
        "ar" => "ara",
        "pl" => "pol",
        "cs" | "cze" => "ces",
        "da" => "dan",
        "fi" => "fin",
        "sv" => "swe",
        "tr" => "tur",
        "uk" => "ukr",
        "he" => "heb",
        "el" | "gre" => "ell",
        value if value.len() == 3 && value.bytes().all(|b| b.is_ascii_lowercase()) => value,
        _ => return Err(failure("unsupported_language")),
    };
    Ok(mapped.into())
}
/// Two-letter tag for a provider code, when the mapping is unambiguous (`fra` -> `fr`).
/// The provider reports no regions, so none is invented.
fn bcp47(code: &str) -> Option<String> {
    Some(
        match code {
            "fra" => "fr",
            "eng" => "en",
            "deu" => "de",
            "spa" => "es",
            "ita" => "it",
            "por" => "pt",
            "jpn" => "ja",
            "zho" => "zh",
            "kor" => "ko",
            "rus" => "ru",
            "nld" => "nl",
            _ => return None,
        }
        .to_owned(),
    )
}
fn languages(config: &ProviderConfig) -> Result<Vec<String>, PluginError> {
    let mut result = vec![code(config.language.as_deref().unwrap_or("en"))?];
    let fallback = config
        .extra
        .get("fallback_language")
        .map(String::as_str)
        .unwrap_or("en");
    if !fallback.trim().is_empty() {
        let fallback = code(fallback)?;
        if !result.contains(&fallback) {
            result.push(fallback);
        }
    }
    Ok(result)
}

async fn http(
    endpoint: &str,
    body: Option<Value>,
    token: Option<&str>,
    config: &ProviderConfig,
) -> Result<Value, PluginError> {
    // Endpoints are built exclusively from validated numeric/IMDb IDs and encoded queries.
    let mut headers: BTreeMap<String, String> = BTreeMap::from([
        ("User-Agent".into(), "Aether TVDB v0.1.0".into()),
        ("Accept".into(), "application/json".into()),
    ]);
    if let Some(token) = token {
        headers.insert("Authorization".into(), format!("Bearer {token}"));
    }
    if body.is_some() {
        headers.insert("Content-Type".into(), "application/json".into());
    }
    let body = body
        .map(|v| serde_json::to_vec(&v).map_err(|_| failure("invalid_input")))
        .transpose()?;
    let url = format!("{API}/{endpoint}");
    #[cfg(target_arch = "wasm32")]
    let (status, bytes) = {
        let response = aether_plugin_api::host_http_fetch(&aether_plugin_api::HostHttpRequest {
            url,
            method: if body.is_some() { "POST" } else { "GET" }.into(),
            headers,
            body,
            timeout_secs: Some(config.timeout_secs.unwrap_or(6).clamp(1, 6) as u32),
        })
        .map_err(|_| failure("network_error"))?;
        (response.status, response.body)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let (status, bytes) = {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(
                config.timeout_secs.unwrap_or(6).clamp(1, 6),
            ))
            .build()
            .map_err(|_| failure("network_error"))?;
        let mut req = if body.is_some() {
            client.post(url)
        } else {
            client.get(url)
        };
        for (key, value) in headers {
            req = req.header(key, value);
        }
        if let Some(body) = body {
            req = req.body(body);
        }
        let mut response = req.send().await.map_err(|_| failure("network_error"))?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| failure("network_error"))?
        {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err(failure("provider_response_too_large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        (status, bytes)
    };
    if bytes.len() > MAX_RESPONSE {
        return Err(failure("provider_response_too_large"));
    }
    match status {
        200..=299 => {}
        401 | 403 => return Err(failure("unauthorized")),
        404 => return Err(failure("not_found")),
        429 | 503 => return Err(failure("rate_limited")),
        300..=399 => return Err(failure("provider_redirect_refused")),
        _ => return Err(failure("provider_error")),
    }
    let result: Value =
        serde_json::from_slice(&bytes).map_err(|_| failure("invalid_provider_response"))?;
    if result["status"].as_str() != Some("success") || result.get("data").is_none() {
        return Err(failure("invalid_provider_response"));
    }
    Ok(result["data"].clone())
}

async fn login(config: &ProviderConfig) -> Result<String, PluginError> {
    let key = config
        .api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| failure("missing_api_key"))?;
    let mut body = json!({"apikey":key});
    match config.extra.get("access_model").map(String::as_str) {
        Some("licensed") => {}
        Some("user_supported") => {
            let pin = config
                .extra
                .get("subscriber_pin")
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| failure("missing_subscriber_pin"))?;
            body["pin"] = json!(pin);
        }
        _ => return Err(failure("access_model_required")),
    }
    let result = http("login", Some(body), None, config).await?;
    result["token"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 8192)
        .map(str::to_owned)
        .ok_or_else(|| failure("invalid_provider_response"))
}

async fn resolve(
    id: &str,
    ids: &BTreeMap<String, String>,
    media: MediaType,
    token: &str,
    config: &ProviderConfig,
) -> Result<String, PluginError> {
    kind(media)?;
    if !id.is_empty() {
        if !identifier(id) {
            return Err(failure("invalid_tvdb_id"));
        }
        return Ok(id.to_string());
    }
    if let Some(id) = ids.get("tvdb") {
        if !identifier(id) {
            return Err(failure("invalid_tvdb_id"));
        }
        return Ok(id.clone());
    }
    let imdb = ids
        .get("imdb")
        .ok_or_else(|| failure("identity_required"))?;
    if !imdb.strip_prefix("tt").is_some_and(identifier) {
        return Err(failure("invalid_external_id"));
    }
    let found = http(
        &format!("search/remoteid/{imdb}"),
        None,
        Some(token),
        config,
    )
    .await?;
    let field = match media {
        MediaType::Movie => "movie",
        MediaType::Episode => "episode",
        _ => "series",
    };
    let values = found
        .as_array()
        .ok_or_else(|| failure("invalid_provider_response"))?;
    let ids: BTreeSet<String> = values
        .iter()
        .filter_map(|v| v[field]["id"].as_u64())
        .filter(|v| *v > 0)
        .map(|v| v.to_string())
        .collect();
    match ids.len() {
        0 => Err(failure("not_found")),
        1 => Ok(ids.into_iter().next().unwrap_or_default()),
        _ => Err(failure("ambiguous_match")),
    }
}

fn metadata(record: &Value, id: &str) -> Result<StructuredMetadata, PluginError> {
    if record["id"].as_u64().map(|v| v.to_string()).as_deref() != Some(id) {
        return Err(failure("identity_mismatch"));
    }
    let mut ids = BTreeMap::from([("tvdb".into(), id.into())]);
    if let Some(remote) = record["remoteIds"].as_array() {
        for value in remote.iter().take(32) {
            let Some(remote) = value["id"].as_str() else {
                continue;
            };
            match value["sourceName"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "imdb" if remote.strip_prefix("tt").is_some_and(identifier) => {
                    ids.entry("imdb".into()).or_insert_with(|| remote.into());
                }
                "themoviedb.com" | "themoviedb" | "tmdb" if identifier(remote) => {
                    ids.entry("tmdb".into()).or_insert_with(|| remote.into());
                }
                _ => {}
            }
        }
    }
    let release = record["firstAired"]
        .as_str()
        .or_else(|| record["aired"].as_str())
        .or_else(|| record["first_release"]["date"].as_str());
    let mut result = StructuredMetadata {
        provider: "tvdb".into(),
        provider_id: id.into(),
        external_ids: ids,
        title: text(record, "name", 128),
        // The API base name does not establish the title's original language.
        original_title: None,
        synopsis: text(record, "overview", 2000),
        year: record["year"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .or_else(|| {
                release
                    .and_then(|v| v.get(..4))
                    .and_then(|v| v.parse().ok())
            }),
        release_date: bounded(release, 10),
        runtime_minutes: record["runtime"]
            .as_u64()
            .or_else(|| record["averageRuntime"].as_u64())
            .filter(|v| *v > 0 && *v <= 10000)
            .map(|v| v as u32),
        status: text(&record["status"], "name", 64),
        ..Default::default()
    };
    if let Some(genres) = record["genres"].as_array() {
        result.genres = genres
            .iter()
            .take(32)
            .filter_map(|v| text(v, "name", 64))
            .collect();
    }
    if let Some(credits) = record["characters"].as_array() {
        for person in credits.iter().take(100) {
            let Some(name) = text(person, "personName", 64) else {
                continue;
            };
            match person["peopleType"]
                .as_str()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str()
            {
                "actor" if result.actors.len() < 32 => result.actors.push(ActorCredit {
                    name,
                    role: text(person, "name", 64),
                    thumb_url: None,
                    order: person["sort"].as_u64().and_then(|v| u32::try_from(v).ok()),
                }),
                "director" if result.directors.len() < 16 && !result.directors.contains(&name) => {
                    result.directors.push(name);
                }
                "writer" if result.writers.len() < 16 && !result.writers.contains(&name) => {
                    result.writers.push(name);
                }
                _ => {}
            }
        }
    }
    // TVDB's score is popularity, never a rating out of ten.
    Ok(result)
}

/// Extra translation languages asked by the Core, as provider codes. One request each, so
/// the list is short: a login, a lookup, the record and the translations must fit the
/// host's ten requests per invocation. Unsupported tags are skipped, never an error.
const MAX_EXTRA_LANGUAGES: usize = 3;

fn extra_languages(
    requested: &[String],
    config: &ProviderConfig,
) -> Result<Vec<String>, PluginError> {
    let known = languages(config)?;
    let mut extra: Vec<String> = Vec::new();
    for tag in requested.iter().take(8) {
        if let Ok(code) = code(tag)
            && !known.contains(&code)
            && !extra.contains(&code)
            && extra.len() < MAX_EXTRA_LANGUAGES
        {
            extra.push(code);
        }
    }
    Ok(extra)
}

async fn details_with_token(
    id: &str,
    ids: &BTreeMap<String, String>,
    media: MediaType,
    token: &str,
    config: &ProviderConfig,
) -> Result<StructuredMetadata, PluginError> {
    details_with_languages(id, ids, media, token, config, &[]).await
}

async fn details_with_languages(
    id: &str,
    ids: &BTreeMap<String, String>,
    media: MediaType,
    token: &str,
    config: &ProviderConfig,
    extra: &[String],
) -> Result<StructuredMetadata, PluginError> {
    let id = resolve(id, ids, media, token, config).await?;
    let endpoint = kind(media)?;
    let record = http(
        &format!("{endpoint}/{id}/extended"),
        None,
        Some(token),
        config,
    )
    .await?;
    let mut result = metadata(&record, &id)?;
    // A reverse IMDb lookup must confirm the identity before automatic application.
    if let Some(imdb) = ids.get("imdb")
        && result.external_ids.get("imdb") != Some(imdb)
    {
        return Err(failure("identity_mismatch"));
    }
    let mut title = false;
    let mut overview = false;
    // Main language and fallback first; the main text comes from the first one that has it.
    let mut wanted = languages(config)?;
    wanted.extend(extra.iter().cloned());
    result.locale = wanted
        .first()
        .map(|l| (*l).clone())
        .and_then(|code| bcp47(&code));
    for language in wanted {
        let translation = match http(
            &format!("{endpoint}/{id}/translations/{language}"),
            None,
            Some(token),
            config,
        )
        .await
        {
            Ok(value) => value,
            Err(e) if e.code == "not_found" => continue,
            Err(e) => return Err(e),
        };
        let item = TranslationItem {
            title: text(&translation, "name", 128),
            overview: text(&translation, "overview", 2000),
            tagline: text(&translation, "tagline", 128),
        };
        if !title && item.title.is_some() {
            result.title = item.title.clone();
            result.tagline = item.tagline.clone();
            title = true;
        }
        if !overview && item.overview.is_some() {
            result.synopsis = item.overview.clone();
            overview = true;
        }
        result.translations.insert(language, item);
        // Stop early only when nobody asked for more languages.
        if title && overview && extra.is_empty() {
            break;
        }
    }
    Ok(result)
}

fn image_url(value: &str) -> Option<String> {
    let path = if value.starts_with("https://artworks.thetvdb.com/") {
        value.to_string()
    } else if value.starts_with("/banners/") {
        format!("https://artworks.thetvdb.com{value}")
    } else {
        return None;
    };
    (path.len() <= 1024 && !path.chars().any(char::is_control)).then_some(path)
}
fn image_role(name: &str) -> Option<ImageType> {
    let name = name.to_ascii_lowercase();
    if name.contains("poster") {
        Some(ImageType::Poster)
    } else if name.contains("background") || name.contains("fanart") {
        Some(ImageType::Backdrop)
    } else if name.contains("clearlogo") || name.contains("clear logo") || name.ends_with("logo") {
        Some(ImageType::Logo)
    } else if name.contains("clearart") || name.contains("clear art") {
        Some(ImageType::ClearArt)
    } else if name.contains("banner") {
        Some(ImageType::Banner)
    } else if name.contains("screencap") || name.contains("thumbnail") {
        Some(ImageType::Thumb)
    } else {
        None
    }
}
fn images_from(
    record: &Value,
    types: &Value,
    media: MediaType,
    config: &ProviderConfig,
) -> Result<Vec<MediaImage>, PluginError> {
    let types = types
        .as_array()
        .ok_or_else(|| failure("invalid_provider_response"))?;
    let roles: BTreeMap<u64, ImageType> = types
        .iter()
        .filter_map(|v| Some((v["id"].as_u64()?, image_role(v["name"].as_str()?)?)))
        .collect();
    let mut artwork = record["artworks"].as_array().cloned().unwrap_or_default();
    if media == MediaType::Episode {
        artwork.push(json!({"image":record["image"],"type":record["imageType"]}));
    }
    let mut seen = BTreeSet::new();
    let mut images = Vec::new();
    for value in artwork.iter().take(200) {
        let Some(url) = value["image"].as_str().and_then(image_url) else {
            continue;
        };
        if !seen.insert(url.clone()) {
            continue;
        }
        let Some(role) = value["type"].as_u64().and_then(|id| roles.get(&id)) else {
            continue;
        };
        let language = value["language"]
            .as_str()
            .filter(|v| !v.is_empty())
            .and_then(|v| code(v).ok());
        images.push(MediaImage {
            url,
            image_type: *role,
            width: value["width"].as_u64().and_then(|v| u32::try_from(v).ok()),
            height: value["height"].as_u64().and_then(|v| u32::try_from(v).ok()),
            language,
            rating: None,
            vote_count: None,
            is_primary: false,
        });
    }
    let mut selected = config.clone();
    selected.language = Some(code(config.language.as_deref().unwrap_or("en"))?);
    selected.fallback_languages = languages(config)?;
    Ok(selected.select_images(images))
}

#[async_trait]
impl MetadataProviderPlugin for TvdbPlugin {
    fn manifest(&self) -> PluginManifest {
        let field = |key: &str, label: &str, secret, required, default: Option<&str>| ConfigField {
            key: key.into(),
            label: label.into(),
            field_type: if secret { "password" } else { "text" }.into(),
            secret,
            required,
            default_value: default.map(str::to_string),
            ..Default::default()
        };
        PluginManifest {api_version:1,id:"tvdb".into(),name:"TheTVDB".into(),version:env!("CARGO_PKG_VERSION").into(),category:"Metadata".into(),provider:"TheTVDB".into(),
            permissions:vec!["metadata.read".into(),"metadata.write".into(),"images".into()],
            http_hosts:vec!["api4.thetvdb.com".into(),"artworks.thetvdb.com".into()],
            description:"TheTVDB API v4 optionnelle. Clé de projet autorisée requise ; PIN personnel si modèle financé par les abonnés.".into(),author:"Aether Team".into(),license:"AGPL-3.0-only".into(),target_abi:"1.0".into(),min_server_version:Some("0.1.0".into()),
            capabilities:vec![PluginCapability::Search,PluginCapability::Details,PluginCapability::Images],supported_media_types:vec![MediaType::Movie,MediaType::TvShow,MediaType::Episode],default_priority:70,
            config_schema:vec![field("api_key","Clé API v4 de projet autorisée",true,true,None),ConfigField {key:"access_model".into(),label:"Modèle d'accès autorisé par TheTVDB".into(),field_type:"select".into(),required:true,options:vec![ConfigOption {label:"Licence de projet approuvée".into(),value:"licensed".into()},ConfigOption {label:"Abonnement utilisateur avec PIN personnel".into(),value:"user_supported".into()}],..Default::default()},field("subscriber_pin","PIN personnel (modèle abonnement uniquement)",true,false,None),field("fallback_language","Langue de repli (vide : valeur de base de l’API)",false,false,Some("en")),aether_plugin_api::provider_timeout_field()],..Default::default()}
    }
    async fn search(
        &self,
        query: SearchQuery,
        config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        kind(query.media_type)?;
        let mut config = config.clone();
        config.language = query.language.clone().or(config.language);
        languages(&config)?;
        let identified =
            query.external_ids.contains_key("tvdb") || query.external_ids.contains_key("imdb");
        if query.media_type == MediaType::Episode && !identified {
            return Err(failure("episode_identity_required"));
        }
        if !identified && (query.title.trim().is_empty() || query.title.len() > 512) {
            return Err(failure("invalid_title"));
        }
        let token = login(&config).await?;
        if identified {
            let metadata =
                details_with_token("", &query.external_ids, query.media_type, &token, &config)
                    .await?;
            return Ok(vec![aether_plugin_api::identified_search_result(
                metadata,
                query.media_type,
            )]);
        }
        let media = if kind(query.media_type)? == "movies" {
            "movie"
        } else {
            "series"
        };
        let mut endpoint = format!(
            "search?limit=20&query={}&type={media}",
            aether_plugin_api::http::urlencode(query.title.trim())
        );
        if let Some(year) = query.year {
            endpoint.push_str(&format!("&year={year}"));
        }
        let data = http(&endpoint, None, Some(&token), &config).await?;
        let items = data
            .as_array()
            .ok_or_else(|| failure("invalid_provider_response"))?;
        let langs = languages(&config)?;
        let mut seen = BTreeSet::new();
        let candidates: Vec<SearchResultItem> = items
            .iter()
            .take(20)
            .filter_map(|v| {
                if v["type"].as_str() != Some(media) {
                    return None;
                }
                let id = v["tvdb_id"].as_str().filter(|v| identifier(v))?;
                if !seen.insert(id.to_string()) {
                    return None;
                }
                let title = langs
                    .iter()
                    .find_map(|lang| text(&v["translations"], lang, 128))
                    .or_else(|| text(v, "name", 128))?;
                let overview = langs
                    .iter()
                    .find_map(|lang| text(&v["overviews"], lang, 1000))
                    .or_else(|| text(v, "overview", 1000));
                Some(SearchResultItem {
                    id: id.into(),
                    title,
                    artist: None,
                    original_title: None,
                    year: v["year"].as_str().and_then(|v| v.parse().ok()),
                    media_type: query.media_type,
                    overview,
                    poster_url: v["image_url"].as_str().and_then(image_url),
                    score: None,
                })
            })
            .collect();
        // Reserve space for the ABI envelope even with long Unicode synopses.
        let mut result = Vec::new();
        let mut bytes = 2;
        for item in candidates {
            let size = serde_json::to_vec(&item)
                .map_err(|_| failure("invalid_provider_response"))?
                .len()
                + 1;
            if bytes + size > 60 * 1024 {
                break;
            }
            bytes += size;
            result.push(item);
        }
        Ok(result)
    }
    async fn details(
        &self,
        request: DetailsRequest,
        config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        kind(request.media_type)?;
        let extra = extra_languages(&request.languages, config)?;
        let token = login(config).await?;
        details_with_languages(
            &request.id,
            &request.external_ids,
            request.media_type,
            &token,
            config,
            &extra,
        )
        .await
    }
    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        kind(request.media_type)?;
        languages(config)?;
        let token = login(config).await?;
        let id = resolve(
            &request.id,
            &request.external_ids,
            request.media_type,
            &token,
            config,
        )
        .await?;
        let record = http(
            &format!("{}/{id}/extended", kind(request.media_type)?),
            None,
            Some(&token),
            config,
        )
        .await?;
        if record["id"].as_u64().map(|v| v.to_string()).as_deref() != Some(&id) {
            return Err(failure("identity_mismatch"));
        }
        let types = http("artwork/types", None, Some(&token), config).await?;
        images_from(&record, &types, request.media_type, config)
    }
}

media_extension_sdk::extension!(handle_tvdb_extension_request);
pub fn handle_tvdb_extension_request(mut req: ExtensionRequest) -> ExtensionResponse {
    // Core-owned library language also applies to details/images, not only searches.
    if let Some(language) = req.input["language"].as_str().map(str::to_owned) {
        if !req.input["config"].is_object() {
            req.input["config"] = json!({});
        }
        req.input["config"]["language"] = json!(language);
    }
    aether_plugin_api::http::handle_extension(&TvdbPlugin, req)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_languages_are_mapped_deduplicated_and_bounded() {
        let config = ProviderConfig {
            language: Some("fr-FR".into()),
            ..Default::default()
        };
        let asked = |tags: &[&str]| tags.iter().map(|t| (*t).to_owned()).collect::<Vec<_>>();
        // French is the main language and English the default fallback: neither is repeated.
        assert_eq!(
            extra_languages(&asked(&["fr-CA", "en-GB", "de", "ja", "es", "it"]), &config).unwrap(),
            ["deu", "jpn", "spa"],
            "at most three extra languages, in the Core's order"
        );
        assert!(
            extra_languages(&asked(&["xx-not a tag", ""]), &config)
                .unwrap()
                .is_empty()
        );
        assert_eq!(bcp47("fra").as_deref(), Some("fr"));
        assert_eq!(bcp47("zzz"), None);
    }

    #[test]
    fn languages_are_explicit_and_bounded() {
        assert_eq!(code("fr-FR").unwrap(), "fra");
        assert_eq!(code("de").unwrap(), "deu");
        assert!(code("../fr").is_err());
        assert!(identifier("42"));
        assert!(!identifier("0"));
        assert!(!identifier("../../42"));
    }
    #[test]
    fn metadata_preserves_ids_and_does_not_mislabel_popularity() {
        let result=metadata(&json!({"id":42,"name":"Fixture","score":94000,"genres":[{"name":"Drama"}],"remoteIds":[{"sourceName":"IMDB","id":"tt0042"}],"characters":[{"peopleType":"Actor","personName":"Actor","name":"Role"},{"peopleType":"Director","personName":"Director"}]}),"42").unwrap();
        assert_eq!(result.external_ids["tvdb"], "42");
        assert_eq!(result.external_ids["imdb"], "tt0042");
        assert_eq!(result.actors.len(), 1);
        assert_eq!(result.directors, ["Director"]);
        assert!(result.ratings.is_empty());
        assert!(metadata(&json!({"id":43}), "42").is_err());
    }
    #[test]
    fn partial_metadata_does_not_invent_values() {
        let result = metadata(&json!({"id":42}), "42").unwrap();
        assert!(result.title.is_none());
        assert!(result.runtime_minutes.is_none());
        assert!(result.year.is_none());
        assert!(result.collection.is_none());
    }
    #[test]
    fn images_use_provider_types_and_exact_host() {
        let types = json!([{"id":9,"name":"Movie Poster"},{"id":10,"name":"Movie Background"}]);
        let record = json!({"artworks":[{"type":9,"image":"https://artworks.thetvdb.com/p.jpg","language":"fra"},{"type":10,"image":"/banners/b.jpg"},{"type":9,"image":"https://evil.example/p.jpg"},{"type":99,"image":"https://artworks.thetvdb.com/unknown.jpg"}]});
        let result = images_from(
            &record,
            &types,
            MediaType::Movie,
            &ProviderConfig {
                language: Some("fr-FR".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].image_type, ImageType::Poster);
        assert_eq!(result[1].image_type, ImageType::Backdrop);
        assert!(image_url("https://artworks.thetvdb.com.evil/p").is_none());
        assert!(image_url("http://artworks.thetvdb.com/p").is_none());
    }
    #[tokio::test]
    async fn credentials_and_episode_scope_fail_before_network() {
        let plugin = TvdbPlugin;
        assert_eq!(
            login(&ProviderConfig::default()).await.unwrap_err().code,
            "missing_api_key"
        );
        let config = ProviderConfig {
            api_key: Some("fixture-secret".into()),
            extra: BTreeMap::from([("access_model".into(), "user_supported".into())]),
            ..Default::default()
        };
        assert_eq!(
            login(&config).await.unwrap_err().code,
            "missing_subscriber_pin"
        );
        assert_eq!(
            plugin
                .search(
                    SearchQuery {
                        title: "Episode".into(),
                        year: None,
                        media_type: MediaType::Episode,
                        language: None,
                        external_ids: BTreeMap::new()
                    },
                    &config
                )
                .await
                .unwrap_err()
                .code,
            "episode_identity_required"
        );
    }
}
