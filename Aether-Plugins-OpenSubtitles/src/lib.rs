//! OpenSubtitles REST adapter. Only the host stores downloaded derivatives.
use aether_plugin_api::{ConfigField, ExtensionRequest, ExtensionResponse, ProviderConfig};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const API: &str = "https://api.opensubtitles.com/api/v1";
const USER_AGENT: &str = "Aether OpenSubtitles v0.1.0";
const MAX_RESPONSE: usize = 262_144;
/// Items returned per page; the Core rejects a larger list.
const MAX_ITEMS: usize = 60;
type Outcome = Result<Value, &'static str>;

pub fn manifest() -> Value {
    let field = |key: &str, label: &str, secret, required| ConfigField {
        key: key.into(),
        label: label.into(),
        secret,
        required,
        field_type: if secret { "password" } else { "text" }.into(),
        ..Default::default()
    };
    json!({"apiVersion":1,"id":"opensubtitles","name":"OpenSubtitles",
        "version":env!("CARGO_PKG_VERSION"),"category":"Subtitles",
        "description":"Recherche et téléchargement explicite de sous-titres. Clé API personnelle requise ; quotas du service applicables.",
        "author":"Aether Team","license":"AGPL-3.0-only",
        "repository":"https://github.com/maelmoreau21/Aether-Plugins",
        "permissions":["subtitles"],
        "httpHosts":["api.opensubtitles.com","vip-api.opensubtitles.com","dl.opensubtitles.com","www.opensubtitles.com"],
        "targetAbi":"1.0","minServerVersion":"0.1.0",
        "configSchema":[field("api_key","Clé API OpenSubtitles",true,true),
            field("username","Compte OpenSubtitles (facultatif)",true,false),
            field("password","Mot de passe OpenSubtitles (facultatif)",true,false),
            aether_plugin_api::provider_timeout_field()]})
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input {
    #[serde(default)]
    title: String,
    year: Option<u16>,
    #[serde(default)]
    media_type: String,
    #[serde(default)]
    external_ids: BTreeMap<String, String>,
    /// Identifiers of the series, for an episode whose own identifiers are unknown.
    #[serde(default)]
    parent_external_ids: BTreeMap<String, String>,
    #[serde(default)]
    language: String,
    forced: Option<bool>,
    sdh: Option<bool>,
    season_number: Option<u32>,
    episode_number: Option<u32>,
    /// OpenSubtitles "moviehash": 16 hexadecimal digits. Never Aether's own fingerprint.
    movie_hash: Option<String>,
    /// Only return files the provider matched by `movie_hash`.
    require_hash: Option<bool>,
    page: Option<u32>,
    file_id: Option<String>,
}

fn numeric(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 16
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.bytes().any(|b| b != b'0')
}

/// A blank value is "not provided", so it never shadows a usable identifier; a non-blank
/// malformed one is an error rather than a silent fallback to a weaker criterion.
fn identifier(
    ids: &BTreeMap<String, String>,
    key: &str,
    imdb_prefix: bool,
) -> Result<Option<String>, &'static str> {
    let Some(raw) = ids.get(key).map(|v| v.trim()).filter(|v| !v.is_empty()) else {
        return Ok(None);
    };
    let value = if imdb_prefix {
        raw.strip_prefix("tt")
            .or_else(|| raw.strip_prefix("TT"))
            .unwrap_or(raw)
    } else {
        raw
    };
    if !numeric(value) {
        return Err("invalid_external_id");
    }
    Ok(Some(value.trim_start_matches('0').to_string()))
}

/// The hash OpenSubtitles asks for is 64 KiB at each end of the file plus its size, as a 64-bit
/// sum rendered as 16 hex digits. Aether's own content fingerprint (`b3s1:<size>:<hex>`, BLAKE3
/// over samples) has another length, alphabet and meaning: it is refused here, never forwarded.
fn movie_hash(raw: &str) -> Result<String, &'static str> {
    let hash = raw.trim().to_ascii_lowercase();
    if hash.len() == 16
        && hash.bytes().all(|b| b.is_ascii_hexdigit())
        && hash.bytes().any(|b| b != b'0')
    {
        Ok(hash)
    } else {
        Err("invalid_movie_hash")
    }
}

/// `pt_BR`, `pt-BR` and `PT-br` all become `pt-br`. A primary code of 2-3 letters, optionally one
/// region/script subtag of 2-4 letters or digits (`pt-br`, `zh-cn`, `zh-hant`, `es-419`).
fn normalize_language(raw: &str) -> Option<String> {
    let value = raw.trim().replace('_', "-").to_ascii_lowercase();
    let mut parts = value.split('-');
    let primary = parts.next()?;
    if !(2..=3).contains(&primary.len()) || !primary.bytes().all(|b| b.is_ascii_lowercase()) {
        return None;
    }
    let mut subtags = 0;
    for part in parts {
        subtags += 1;
        if subtags > 1
            || !(2..=4).contains(&part.len())
            || !part.bytes().all(|b| b.is_ascii_alphanumeric())
        {
            return None;
        }
    }
    Some(value)
}

/// A request for `pt-br` accepts only `pt-br`; a request for `pt` accepts every Portuguese
/// variant; the provider's own spelling (`pt-BR`, `pt_BR`) is irrelevant.
fn language_matches(requested: &str, found: &str) -> bool {
    found == requested || (!requested.contains('-') && found.split('-').next() == Some(requested))
}

/// The Core only admits alphabetic subtags (`es-419` would invalidate the whole page), so a
/// numeric region is reduced to its primary language.
fn public_language(found: &str) -> &str {
    if found
        .split('-')
        .all(|p| p.bytes().all(|b| b.is_ascii_alphabetic()))
    {
        found
    } else {
        found.split('-').next().unwrap_or(found)
    }
}

fn search_query(input: &Input) -> Result<(String, &'static str), &'static str> {
    let language = normalize_language(&input.language).ok_or("invalid_language")?;
    let media = match input.media_type.as_str() {
        "movie" => "movie",
        "episode" => "episode",
        _ => return Err("unsupported_media_type"),
    };
    let hash = match input.movie_hash.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => Some(movie_hash(raw)?),
    };
    if input.require_hash == Some(true) && hash.is_none() {
        return Err("movie_hash_required");
    }
    let imdb = identifier(&input.external_ids, "imdb", true)?;
    let tmdb = identifier(&input.external_ids, "tmdb", false)?;
    let (parent_imdb, parent_tmdb) = if media == "episode" {
        (
            identifier(&input.parent_external_ids, "imdb", true)?,
            identifier(&input.parent_external_ids, "tmdb", false)?,
        )
    } else {
        (None, None)
    };
    let mut query = BTreeMap::<&str, String>::new();
    query.insert("languages", language);
    query.insert("type", media.into());
    if let Some(hash) = &hash {
        query.insert("moviehash", hash.clone());
        if input.require_hash == Some(true) {
            query.insert("moviehash_match", "only".into());
        }
    }
    // An episode's own identifier or file hash designates it; otherwise it can only be found
    // through its series and numbers, which must then both be present.
    let episode_identified = imdb.is_some() || tmdb.is_some() || hash.is_some();
    if media == "episode"
        && !episode_identified
        && (input.season_number.is_none() || input.episode_number.is_none())
    {
        return Err("season_episode_required");
    }
    let identified = if let Some(imdb) = imdb {
        query.insert("imdb_id", imdb);
        true
    } else if let Some(tmdb) = tmdb {
        query.insert("tmdb_id", tmdb);
        true
    } else if let Some(imdb) = parent_imdb {
        query.insert("parent_imdb_id", imdb);
        true
    } else if let Some(tmdb) = parent_tmdb {
        query.insert("parent_tmdb_id", tmdb);
        true
    } else {
        hash.is_some()
    };
    if !identified {
        if input.title.trim().is_empty() || input.title.len() > 512 {
            return Err("invalid_title");
        }
        query.insert("query", input.title.trim().to_lowercase());
        // The work's year is the series' for an episode and would exclude every other season.
        if let (Some(year), "movie") = (input.year, media) {
            query.insert("year", year.to_string());
        }
    }
    if let Some(forced) = input.forced {
        query.insert(
            "foreign_parts_only",
            if forced { "only" } else { "exclude" }.into(),
        );
    }
    if let Some(sdh) = input.sdh {
        query.insert(
            "hearing_impaired",
            if sdh { "only" } else { "exclude" }.into(),
        );
    }
    if media == "episode" {
        for (key, value) in [
            ("season_number", input.season_number),
            ("episode_number", input.episode_number),
        ] {
            match value {
                Some(value) if value > 9999 => return Err("invalid_episode_number"),
                Some(value) => {
                    query.insert(key, value.to_string());
                }
                None => {}
            }
        }
    }
    if let Some(page) = input.page {
        if !(1..=100).contains(&page) {
            return Err("invalid_page");
        }
        if page > 1 {
            query.insert("page", page.to_string());
        }
    }
    Ok((
        query
            .into_iter()
            .map(|(k, v)| {
                format!(
                    "{k}={}",
                    aether_plugin_api::http::urlencode(&v).replace("%20", "+")
                )
            })
            .collect::<Vec<_>>()
            .join("&"),
        if identified { "identity" } else { "title" },
    ))
}

/// What the caller asked for. The provider is trusted to filter, never to be right about it.
struct Wanted<'a> {
    language: &'a str,
    forced: Option<bool>,
    sdh: Option<bool>,
}

fn clean_label(text: Option<&str>) -> Option<String> {
    let label: String = text?
        .chars()
        .filter(|c| !c.is_control())
        .take(128)
        .collect();
    let label = label.trim();
    (!label.is_empty()).then(|| label.to_string())
}

fn search_result(value: Value, basis: &str, wanted: &Wanted) -> Outcome {
    let data = value["data"]
        .as_array()
        .ok_or("invalid_provider_response")?;
    let mut items: Vec<Value> = Vec::new();
    let mut hash_matches = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for result in data.iter().take(MAX_ITEMS) {
        let a = &result["attributes"];
        let Some(found) = a["language"]
            .as_str()
            .map(|v| v.trim().replace('_', "-").to_ascii_lowercase())
            .filter(|v| !v.is_empty() && v.len() <= 16)
        else {
            continue;
        };
        let forced = a["foreign_parts_only"].as_bool().unwrap_or(false);
        let sdh = a["hearing_impaired"].as_bool().unwrap_or(false);
        if !language_matches(wanted.language, &found)
            || wanted.forced.is_some_and(|w| w != forced)
            || wanted.sdh.is_some_and(|w| w != sdh)
        {
            continue;
        }
        let Some(files) = a["files"].as_array() else {
            continue;
        };
        for file in files.iter().take(8) {
            let Some(id) = file["file_id"].as_u64().filter(|id| *id > 0) else {
                continue;
            };
            // The Core refuses the whole page when an identifier repeats.
            if !seen.insert(id) {
                continue;
            }
            let label = clean_label(a["release"].as_str())
                .or_else(|| clean_label(file["file_name"].as_str()))
                .unwrap_or_else(|| "OpenSubtitles".into());
            hash_matches.push(a["moviehash_match"].as_bool().unwrap_or(false));
            items.push(
                json!({"id":id.to_string(),"language":public_language(&found),"label":label,
                "forced":forced,"sdh":sdh,"match":basis,
                "hashMatch":hash_matches.last(),
                "machineTranslated":a["machine_translated"].as_bool().unwrap_or(false),
                "aiTranslated":a["ai_translated"].as_bool().unwrap_or(false),
                "downloadCount":a["download_count"].as_u64().unwrap_or(0)}),
            );
        }
    }
    // Files matching the caller's own video come first; the provider's order is kept otherwise.
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|i| !hash_matches[*i]);
    let items: Vec<Value> = order
        .into_iter()
        .take(MAX_ITEMS)
        .map(|i| items[i].clone())
        .collect();
    Ok(json!({"items":items,"totalPages":value["total_pages"].as_u64().unwrap_or(1).clamp(1, 100)}))
}

fn download_result(value: Value) -> Outcome {
    let url = value["link"]
        .as_str()
        .filter(|v| v.len() <= 4096)
        .ok_or("invalid_provider_response")?;
    // Full URL parsing/DNS pinning is performed again by the host before fetching.
    if ![
        "https://dl.opensubtitles.com/",
        "https://www.opensubtitles.com/",
    ]
    .iter()
    .any(|prefix| url.starts_with(prefix))
        || url
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '\\')
    {
        return Err("download_host_denied");
    }
    let mut result = json!({"url":url});
    if let Some(remaining) = value["remaining"].as_u64() {
        result["remaining"] = json!(remaining);
    }
    if let Some(reset) = value["reset_time_utc"].as_str().filter(|v| v.len() <= 80) {
        result["resetTimeUtc"] = json!(reset);
    }
    Ok(result)
}

#[derive(Clone, Copy, PartialEq)]
enum Call {
    Login,
    Search,
    Download,
}

/// Documented statuses, mapped to the codes the other providers use. Nothing is retried: a
/// download consumes allowance even when the answer is lost, and the Core keeps the cooldown.
fn status_code(status: u16, call: Call) -> Option<&'static str> {
    Some(match status {
        200..=299 => return None,
        401 if call == Call::Login => "invalid_credentials",
        401 | 403 => "unauthorized",
        404 | 410 => "not_found",
        400 | 422 => "invalid_request",
        406 if call == Call::Download => "download_quota_exceeded",
        408 => "timeout",
        429 | 503 => "rate_limited",
        502 | 504 => "provider_unavailable",
        300..=399 => "provider_redirect_refused",
        _ => "provider_error",
    })
}

/// Broker refusals keep their meaning instead of all becoming `network_error`.
#[cfg(target_arch = "wasm32")]
fn broker_code(code: &str) -> &'static str {
    match code {
        "dns_timeout" | "timeout" => "timeout",
        "quota_exceeded" => "quota_exceeded",
        "provider_rate_limited" => "rate_limited",
        "response_too_large" => "provider_response_too_large",
        "domain_not_allowed" => "domain_not_allowed",
        _ => "network_error",
    }
}

async fn request(
    url: &str,
    body: Option<Value>,
    key: &str,
    token: Option<&str>,
    config: &ProviderConfig,
    call: Call,
) -> Outcome {
    let mut headers: BTreeMap<String, String> = BTreeMap::from([
        ("Api-Key".into(), key.into()),
        ("User-Agent".into(), USER_AGENT.into()),
        ("Accept".into(), "application/json".into()),
    ]);
    if let Some(token) = token {
        headers.insert("Authorization".into(), format!("Bearer {token}"));
    }
    if body.is_some() {
        headers.insert("Content-Type".into(), "application/json".into());
    }
    let body = body
        .map(|v| serde_json::to_vec(&v).map_err(|_| "invalid_input"))
        .transpose()?;
    #[cfg(target_arch = "wasm32")]
    let (status, bytes) = {
        let response = aether_plugin_api::host_http_fetch(&aether_plugin_api::HostHttpRequest {
            url: url.into(),
            method: if body.is_some() { "POST" } else { "GET" }.into(),
            headers,
            body,
            timeout_secs: Some(config.timeout_secs.unwrap_or(6).clamp(1, 6) as u32),
        })
        .map_err(|error| broker_code(&error.code))?;
        (response.status, response.body)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let (status, bytes) = {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(
                config.timeout_secs.unwrap_or(6).clamp(1, 6),
            ))
            .build()
            .map_err(|_| "network_error")?;
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
        let mut response = req.send().await.map_err(|e| {
            if e.is_timeout() {
                "timeout"
            } else {
                "network_error"
            }
        })?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| {
            if e.is_timeout() {
                "timeout"
            } else {
                "truncated_response"
            }
        })? {
            if bytes.len() + chunk.len() > MAX_RESPONSE {
                return Err("provider_response_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        (status, bytes)
    };
    if bytes.len() > MAX_RESPONSE {
        return Err("provider_response_too_large");
    }
    if let Some(code) = status_code(status, call) {
        return Err(code);
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        if e.is_eof() {
            "truncated_response"
        } else {
            "invalid_provider_response"
        }
    })
}

pub async fn run(req: ExtensionRequest) -> ExtensionResponse {
    let result = run_inner(&req)
        .await
        .unwrap_or_else(|code| json!({"error":code}));
    ExtensionResponse {
        api_version: 1,
        result,
    }
}

/// The API root. Only a native run (tests, tooling) may be pointed elsewhere; the Wasm module
/// never leaves `api.opensubtitles.com` / `vip-api.opensubtitles.com`.
fn api_root(config: &ProviderConfig) -> String {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(base) = &config.base_url {
        return base.trim_end_matches('/').to_string();
    }
    let _ = config;
    API.to_string()
}

async fn run_inner(req: &ExtensionRequest) -> Outcome {
    if req.api_version != 1 {
        return Err("unsupported_api_version");
    }
    if req.operation == "manifest" {
        return Ok(manifest());
    }
    if !matches!(
        req.operation.as_str(),
        "subtitles.search" | "subtitles.download"
    ) {
        return Err("unsupported_operation");
    }
    let config = ProviderConfig::from_extension_input(&req.input);
    let key = config
        .api_key
        .as_deref()
        .filter(|k| !k.trim().is_empty())
        .ok_or("missing_api_key")?;
    let input: Input = serde_json::from_value(req.input.clone()).map_err(|_| "invalid_input")?;
    // Validate everything before spending a request or a download allowance.
    let query = if req.operation == "subtitles.search" {
        Some(search_query(&input)?)
    } else {
        None
    };
    let file_id = if req.operation == "subtitles.download" {
        let id = input
            .file_id
            .as_deref()
            .filter(|v| numeric(v))
            .ok_or("invalid_file_id")?;
        Some(
            id.parse::<u64>()
                .ok()
                .filter(|v| *v > 0)
                .ok_or("invalid_file_id")?,
        )
    } else {
        None
    };
    let username = config
        .extra
        .get("username")
        .filter(|v| !v.trim().is_empty());
    let password = config.extra.get("password").filter(|v| !v.is_empty());
    let root = api_root(&config);
    let (base, token) = match (username, password) {
        (None, None) => (root, None),
        (Some(username), Some(password)) => {
            let login = request(
                &format!("{root}/login"),
                Some(json!({"username":username,"password":password})),
                key,
                None,
                &config,
                Call::Login,
            )
            .await?;
            let host = login["base_url"]
                .as_str()
                .unwrap_or("api.opensubtitles.com");
            if !matches!(host, "api.opensubtitles.com" | "vip-api.opensubtitles.com") {
                return Err("api_host_denied");
            }
            let token = login["token"]
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 8192)
                .ok_or("invalid_provider_response")?;
            (format!("https://{host}/api/v1"), Some(token.to_string()))
        }
        _ => return Err("incomplete_credentials"),
    };
    if let Some((query, basis)) = query {
        let language = normalize_language(&input.language).ok_or("invalid_language")?;
        let wanted = Wanted {
            language: &language,
            forced: input.forced,
            sdh: input.sdh,
        };
        search_result(
            request(
                &format!("{base}/subtitles?{query}"),
                None,
                key,
                token.as_deref(),
                &config,
                Call::Search,
            )
            .await?,
            basis,
            &wanted,
        )
    } else {
        download_result(
            request(
                &format!("{base}/download"),
                Some(json!({"file_id":file_id,"sub_format":"srt"})),
                key,
                token.as_deref(),
                &config,
                Call::Download,
            )
            .await?,
        )
    }
}

media_extension_sdk::extension!(handle_extension);
pub fn handle_extension(req: ExtensionRequest) -> ExtensionResponse {
    aether_plugin_api::http::block_on_guest(run(req))
}

#[cfg(test)]
mod tests;
