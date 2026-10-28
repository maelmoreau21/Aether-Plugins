//! Shared helpers for provider plugins: brokered/native HTTP, URL encoding, guest executor.

use crate::{
    DetailsRequest, ExtensionRequest, ExtensionResponse, ImagesRequest, MetadataProviderPlugin,
    PluginError, ProviderConfig, SearchQuery,
};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

/// Percent-encode a string for use in a URL query component (RFC 3986 unreserved kept).
pub fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Map a provider HTTP status to a typed error (`None` when the status is a success).
pub fn status_error(status: u16, provider: &str, what: &str) -> Option<PluginError> {
    match status {
        200..=299 => None,
        404 => Some(PluginError::not_found(what)),
        429 => Some(PluginError::rate_limited(10)),
        503 => Some(PluginError::rate_limited(5)),
        401 | 403 => Some(PluginError::new(
            "unauthorized",
            format!("{provider} rejected the credentials (HTTP {status})"),
        )),
        _ => Some(PluginError::new(
            "provider_error",
            format!("{provider} returned HTTP status {status}"),
        )),
    }
}

/// Minimum spacing between requests to a rate-limited provider (native execution only).
///
/// Reserves the next free slot for key and waits for it, so concurrent callers are spaced out.
/// On wasm32 guests there is no clock or timer: the Aether host broker must enforce provider
/// limits there, and providers answering 429/503 are mapped to `rate_limited`.
#[cfg(not(target_arch = "wasm32"))]
pub async fn throttle(key: &str, min_interval: std::time::Duration) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;
    static SLOTS: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    let wait = {
        let mut slots = SLOTS
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let slot = slots
            .get(key)
            .map(|last| (*last + min_interval).max(now))
            .unwrap_or(now);
        slots.insert(key.to_string(), slot);
        slot.saturating_duration_since(now)
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
}

/// No-op on wasm32 (see the native variant).
#[cfg(target_arch = "wasm32")]
pub async fn throttle(_key: &str, _min_interval: std::time::Duration) {}

/// Perform a GET request and return the parsed JSON body.
///
/// On `wasm32` the request goes through the Aether host broker (restricted to the manifest
/// `http_hosts`); natively it uses `reqwest`. Error messages never contain the request URL,
/// because it may embed an API key.
pub async fn get_json(
    url: &str,
    headers: &[(&str, &str)],
    config: &ProviderConfig,
    provider: &str,
    what: &str,
) -> Result<serde_json::Value, PluginError> {
    #[cfg(target_arch = "wasm32")]
    {
        let mut map = std::collections::BTreeMap::new();
        map.insert("Accept".to_string(), "application/json".to_string());
        for (k, v) in headers {
            map.insert((*k).to_string(), (*v).to_string());
        }
        let request = crate::HostHttpRequest {
            url: url.to_string(),
            method: "GET".into(),
            headers: map,
            body: None,
            timeout_secs: config.timeout_secs.map(|s| s as u32),
        };
        let resp =
            crate::host_http_fetch(&request).map_err(|e| PluginError::new(e.code, e.message))?;
        if let Some(err) = status_error(resp.status, provider, what) {
            return Err(err);
        }
        serde_json::from_slice(&resp.body)
            .map_err(|e| PluginError::new("parse_error", e.to_string()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let timeout = std::time::Duration::from_secs(config.timeout_secs.unwrap_or(10));
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| PluginError::new("client_error", e.without_url().to_string()))?;
        let mut req = client.get(url).header("Accept", "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let res = req
            .send()
            .await
            .map_err(|e| PluginError::new("network_error", e.without_url().to_string()))?;
        if let Some(err) = status_error(res.status().as_u16(), provider, what) {
            return Err(err);
        }
        res.json()
            .await
            .map_err(|e| PluginError::new("parse_error", e.without_url().to_string()))
    }
}

/// Lightweight synchronous executor for guest WebAssembly (host HTTP calls complete inline).
pub fn block_on_guest<F: std::future::Future>(future: F) -> F::Output {
    static VTABLE: RawWakerVTable =
        RawWakerVTable::new(|p| RawWaker::new(p, &VTABLE), |_| {}, |_| {}, |_| {});
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
    let mut cx = Context::from_waker(&waker);
    let mut pinned = std::pin::pin!(future);
    match pinned.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("Guest Wasm async operation did not complete synchronously"),
    }
}

fn error_result(error: &PluginError) -> serde_json::Value {
    serde_json::json!({"error": error.code, "message": error.message})
}

/// Promote the credited artist returned by a music provider to the optional
/// structured search field expected by recent Core versions. Call only for
/// providers whose search `overview` is explicitly the credited artist.
pub fn promote_music_artist(response: &mut ExtensionResponse) {
    let Some(items) = response.result.as_array_mut() else {
        return;
    };
    for item in items {
        if item["mediaType"].as_str() != Some("music") {
            continue;
        }
        let artist = item["overview"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned);
        if let Some(artist) = artist {
            item["artist"] = serde_json::json!(artist);
        }
    }
}

/// Generic guest entry point shared by metadata provider plugins.
pub fn handle_extension<P: MetadataProviderPlugin>(
    plugin: &P,
    req: ExtensionRequest,
) -> ExtensionResponse {
    if req.api_version != crate::API_VERSION {
        return ExtensionResponse {
            api_version: crate::API_VERSION,
            result: serde_json::json!({"error": "unsupported_api_version"}),
        };
    }
    let config = ProviderConfig::from_extension_input(&req.input);
    let result = match req.operation.as_str() {
        "manifest" => serde_json::to_value(plugin.manifest()).unwrap_or_default(),
        "metadata.search" | "search" => match serde_json::from_value::<SearchQuery>(req.input) {
            Ok(q) => match block_on_guest(plugin.search(q, &config)) {
                Ok(v) => serde_json::to_value(v).unwrap_or_default(),
                Err(e) => error_result(&e),
            },
            Err(_) => serde_json::json!({"error": "invalid_search_input"}),
        },
        "metadata.details" | "details" => {
            match serde_json::from_value::<DetailsRequest>(req.input) {
                Ok(d) => match block_on_guest(plugin.details(d, &config)) {
                    Ok(v) => serde_json::to_value(v).unwrap_or_default(),
                    Err(e) => error_result(&e),
                },
                Err(_) => serde_json::json!({"error": "invalid_details_input"}),
            }
        }
        "metadata.images" | "images" => match serde_json::from_value::<ImagesRequest>(req.input) {
            Ok(i) => match block_on_guest(plugin.images(i, &config)) {
                Ok(v) => serde_json::to_value(v).unwrap_or_default(),
                Err(e) => error_result(&e),
            },
            Err(_) => serde_json::json!({"error": "invalid_images_input"}),
        },
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
                    .unwrap_or(crate::MediaType::Movie),
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
                        let proposal = crate::MetadataProposal {
                            title: first.title.clone(),
                            summary: first.overview.clone().unwrap_or_default(),
                        };
                        serde_json::to_value(proposal).unwrap_or_default()
                    } else {
                        serde_json::json!({"error": if items.is_empty() { "not_found" } else { "ambiguous_match" }})
                    }
                }
                Err(e) => error_result(&e),
            }
        }
        _ => serde_json::json!({"error": "unsupported_operation"}),
    };
    ExtensionResponse {
        api_version: crate::API_VERSION,
        result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_escapes_reserved_and_utf8() {
        assert_eq!(urlencode("a b&c=é"), "a%20b%26c%3D%C3%A9");
        assert_eq!(urlencode("Az09-_.~"), "Az09-_.~");
    }

    #[tokio::test]
    async fn throttle_spaces_requests_per_key() {
        let gap = std::time::Duration::from_millis(120);
        let start = std::time::Instant::now();
        throttle("test-key", gap).await;
        throttle("test-key", gap).await;
        throttle("test-key", gap).await;
        assert!(start.elapsed() >= gap * 2);
        let other = std::time::Instant::now();
        throttle("other-key", gap).await;
        assert!(other.elapsed() < gap);
    }

    #[test]
    fn status_mapping_is_typed() {
        assert!(status_error(200, "p", "x").is_none());
        assert_eq!(status_error(404, "p", "x").unwrap().code, "not_found");
        assert_eq!(status_error(429, "p", "x").unwrap().code, "rate_limited");
        assert_eq!(status_error(401, "p", "x").unwrap().code, "unauthorized");
        assert_eq!(status_error(500, "p", "x").unwrap().code, "provider_error");
    }
}
