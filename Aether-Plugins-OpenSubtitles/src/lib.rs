//! OpenSubtitles REST adapter. Only the host stores downloaded derivatives.
use aether_plugin_api::{ConfigField, ExtensionRequest, ExtensionResponse, ProviderConfig};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const API: &str = "https://api.opensubtitles.com/api/v1";
const USER_AGENT: &str = "Aether OpenSubtitles v0.1.0";
const MAX_RESPONSE: usize = 262_144;
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
    #[serde(default)]
    language: String,
    forced: Option<bool>,
    sdh: Option<bool>,
    season_number: Option<u32>,
    episode_number: Option<u32>,
    page: Option<u32>,
    file_id: Option<String>,
}

fn numeric(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 16
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.bytes().any(|b| b != b'0')
}

fn search_query(input: &Input) -> Result<(String, &'static str), &'static str> {
    if input.language.is_empty()
        || input.language.len() > 16
        || !input
            .language
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c == b'-')
    {
        return Err("invalid_language");
    }
    let mut query = BTreeMap::<&str, String>::new();
    query.insert("languages", input.language.clone());
    let identity = if let Some(imdb) = input.external_ids.get("imdb") {
        let value = imdb.strip_prefix("tt").unwrap_or(imdb);
        if !numeric(value) {
            return Err("invalid_external_id");
        }
        query.insert("imdb_id", value.trim_start_matches('0').to_string());
        true
    } else if let Some(tmdb) = input.external_ids.get("tmdb") {
        if !numeric(tmdb) {
            return Err("invalid_external_id");
        }
        query.insert("tmdb_id", tmdb.trim_start_matches('0').to_string());
        true
    } else {
        if input.title.trim().is_empty() || input.title.len() > 512 {
            return Err("invalid_title");
        }
        query.insert("query", input.title.trim().to_lowercase());
        if let Some(year) = input.year {
            query.insert("year", year.to_string());
        }
        false
    };
    query.insert(
        "type",
        match input.media_type.as_str() {
            "movie" => "movie".into(),
            "episode" => "episode".into(),
            _ => return Err("unsupported_media_type"),
        },
    );
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
    for (key, value) in [
        ("season_number", input.season_number),
        ("episode_number", input.episode_number),
    ] {
        if let Some(value) = value {
            query.insert(key, value.to_string());
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
        if identity { "identity" } else { "title" },
    ))
}

fn search_result(value: Value, basis: &str) -> Outcome {
    let data = value["data"]
        .as_array()
        .ok_or("invalid_provider_response")?;
    let mut items = Vec::new();
    for result in data.iter().take(60) {
        let a = &result["attributes"];
        let Some(language) = a["language"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 16)
        else {
            continue;
        };
        let Some(files) = a["files"].as_array() else {
            continue;
        };
        for file in files.iter().take(8) {
            let Some(id) = file["file_id"].as_u64().filter(|id| *id > 0) else {
                continue;
            };
            let label: String = a["release"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| file["file_name"].as_str())
                .unwrap_or("OpenSubtitles")
                .chars()
                .filter(|c| !c.is_control())
                .take(128)
                .collect();
            items.push(
                json!({"id":id.to_string(),"language":language,"label":label,
                "forced":a["foreign_parts_only"].as_bool().unwrap_or(false),
                "sdh":a["hearing_impaired"].as_bool().unwrap_or(false),
                "match":basis,"downloadCount":a["download_count"].as_u64().unwrap_or(0)}),
            );
            if items.len() >= 60 {
                break;
            }
        }
        if items.len() >= 60 {
            break;
        }
    }
    Ok(json!({"items":items,"totalPages":value["total_pages"].as_u64().unwrap_or(1).min(100)}))
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
        || url.chars().any(char::is_control)
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

async fn request(
    url: &str,
    body: Option<Value>,
    key: &str,
    token: Option<&str>,
    config: &ProviderConfig,
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
        .map_err(|_| "network_error")?;
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
        let mut response = req.send().await.map_err(|_| "network_error")?;
        let status = response.status().as_u16();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "network_error")? {
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
    match status {
        200..=299 => serde_json::from_slice(&bytes).map_err(|_| "invalid_provider_response"),
        401 | 403 => Err("unauthorized"),
        406 => Err("download_quota_exceeded"),
        429 | 503 => Err("rate_limited"),
        300..=399 => Err("provider_redirect_refused"),
        _ => Err("provider_error"),
    }
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
    let (base, token) = match (username, password) {
        (None, None) => (API.to_string(), None),
        (Some(username), Some(password)) => {
            let login = request(
                &format!("{API}/login"),
                Some(json!({"username":username,"password":password})),
                key,
                None,
                &config,
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
        search_result(
            request(
                &format!("{base}/subtitles?{query}"),
                None,
                key,
                token.as_deref(),
                &config,
            )
            .await?,
            basis,
        )
    } else {
        download_result(
            request(
                &format!("{base}/download"),
                Some(json!({"file_id":file_id,"sub_format":"srt"})),
                key,
                token.as_deref(),
                &config,
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
mod tests {
    use super::*;
    fn input(value: Value) -> Input {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn identity_query_does_not_guess_title_or_hash() {
        let (query,basis)=search_query(&input(json!({"title":"Wrong title","year":2020,"mediaType":"movie","language":"fr","externalIds":{"imdb":"tt0133093"},"forced":true,"sdh":false}))).unwrap();
        assert_eq!(basis, "identity");
        assert_eq!(
            query,
            "foreign_parts_only=only&hearing_impaired=exclude&imdb_id=133093&languages=fr&type=movie"
        );
        assert!(!query.contains("moviehash"));
    }
    #[test]
    fn fallback_query_and_invalid_inputs() {
        let (query, basis) = search_query(&input(
            json!({"title":"A Film","year":1999,"mediaType":"movie","language":"en"}),
        ))
        .unwrap();
        assert_eq!(basis, "title");
        assert!(query.contains("query=a+film"));
        assert!(query.contains("year=1999"));
        for value in [
            json!({"mediaType":"movie","language":"../en"}),
            json!({"title":"x","mediaType":"series","language":"fr"}),
            json!({"title":"x","mediaType":"movie","language":"fr","page":0}),
        ] {
            assert!(search_query(&input(value)).is_err());
        }
    }
    #[test]
    fn search_flattens_files_and_preserves_flags() {
        let result=search_result(json!({"total_pages":3,"data":[{"attributes":{"language":"fr","foreign_parts_only":true,"hearing_impaired":true,"release":"Release","files":[{"file_id":42},{"file_id":43}]}}]}),"identity").unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 2);
        assert_eq!(result["items"][0]["id"], "42");
        assert_eq!(result["items"][0]["forced"], true);
        assert_eq!(result["items"][0]["sdh"], true);
        assert!(search_result(json!({"secret":"never returned"}), "title").is_err());
    }
    #[test]
    fn download_hosts_and_quotas_are_validated() {
        let output=download_result(json!({"link":"https://dl.opensubtitles.com/download/opaque","remaining":0,"reset_time_utc":"2026-11-04T00:00:00Z","token":"secret"})).unwrap();
        assert_eq!(output["remaining"], 0);
        assert!(output.get("token").is_none());
        for link in [
            "http://dl.opensubtitles.com/x",
            "https://dl.opensubtitles.com.evil/x",
            "https://127.0.0.1/x",
            "https://dl.opensubtitles.com@evil/x",
        ] {
            assert!(download_result(json!({"link":link})).is_err());
        }
    }
    #[tokio::test]
    async fn invalid_calls_never_need_network_or_reveal_secrets() {
        let req = |op: &str, input| ExtensionRequest {
            api_version: 1,
            operation: op.into(),
            input,
        };
        assert_eq!(
            run(req("subtitles.search", json!({}))).await.result,
            json!({"error":"missing_api_key"})
        );
        let result = run(req(
            "subtitles.download",
            json!({"fileId":"../secret","config":{"api_key":"fake-secret"}}),
        ))
        .await
        .result;
        assert_eq!(result, json!({"error":"invalid_file_id"}));
        assert!(!result.to_string().contains("fake-secret"));
    }
}
