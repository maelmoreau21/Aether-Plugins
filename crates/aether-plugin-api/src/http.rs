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

/// Largest provider body accepted. Mirrors the Core broker (`DEFAULT_MAX_RESPONSE_SIZE`), so a
/// native run fails the same way a Wasm run does instead of buffering an unbounded body.
pub const MAX_RESPONSE_BYTES: usize = 262_144;

/// Spacing hint reported with a 429 that carries no usable `Retry-After`.
const DEFAULT_RATE_LIMIT_HINT_SECS: u64 = 10;

/// Parse a `Retry-After` header given as delta-seconds, bounded to one second..one day.
///
/// The HTTP-date form is deliberately not interpreted: a Wasm guest has no clock, and the Core
/// broker already turns the date into the durable provider cooldown. Such a value yields `None`.
pub fn retry_after_secs(value: Option<&str>) -> Option<u64> {
    let value = value?.trim();
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok().map(|s| s.clamp(1, 86_400))
}

/// `Retry-After` of a brokered response (the broker lower-cases header names).
pub fn retry_after_from_headers(
    headers: &std::collections::BTreeMap<String, String>,
) -> Option<u64> {
    retry_after_secs(
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
            .map(|(_, value)| value.as_str()),
    )
}

/// Map a provider HTTP status to a typed error (`None` when the status is a success).
///
/// | status | code | meaning |
/// |---|---|---|
/// | 401 | `unauthorized` | credentials missing or invalid |
/// | 403 | `forbidden` | credentials valid but access refused (plan, licence, IP) |
/// | 404, 410 | `not_found` | the work does not exist |
/// | 408 | `timeout` | the provider gave up waiting |
/// | 429 | `rate_limited` | quota reached, `retry_after_secs` from `Retry-After` |
/// | 502-504 | `provider_unavailable` | provider outage, `retry_after_secs` when announced |
/// | 3xx | `provider_redirect_refused` | the broker never follows redirects |
/// | other | `provider_error` | anything else |
///
/// Nothing here retries: the SDK performs exactly one request per call.
pub fn classify_status(
    status: u16,
    retry_after: Option<u64>,
    provider: &str,
    what: &str,
) -> Option<PluginError> {
    // One `format!` for every class keeps the Wasm modules small (they sit close to the
    // runtime's module size limit).
    let (code, summary) = match status {
        200..=299 => return None,
        404 | 410 => return Some(PluginError::not_found(what)),
        429 => {
            return Some(PluginError::rate_limited(
                retry_after.unwrap_or(DEFAULT_RATE_LIMIT_HINT_SECS),
            ));
        }
        300..=399 => ("provider_redirect_refused", "answered with a redirect"),
        401 => ("unauthorized", "rejected the credentials"),
        403 => ("forbidden", "refused access with these credentials"),
        408 => ("timeout", "timed out"),
        502..=504 => ("provider_unavailable", "is unavailable"),
        _ => ("provider_error", "returned an unexpected status"),
    };
    let mut error = PluginError::new(code, format!("{provider} {summary} (HTTP {status})"));
    if code == "provider_unavailable" {
        error.retry_after_secs = retry_after;
    }
    Some(error)
}

/// [`classify_status`] without a `Retry-After` value.
pub fn status_error(status: u16, provider: &str, what: &str) -> Option<PluginError> {
    classify_status(status, None, provider, what)
}

/// Convert a Core broker error. Codes are bounded and kept (`quota_exceeded`,
/// `domain_not_allowed`, `response_too_large`...); two are normalised so plugins can match one
/// vocabulary: `dns_timeout` becomes `timeout` and the broker cooldown `provider_rate_limited`
/// becomes `rate_limited`. The broker message is built from the request error and may embed the
/// request URL, API key included, so it is dropped: the code says what happened.
pub fn host_error(error: crate::HostHttpError) -> PluginError {
    let code: String = match error.code.as_str() {
        "dns_timeout" => "timeout".into(),
        "provider_rate_limited" => "rate_limited".into(),
        other => other
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            .take(64)
            .collect(),
    };
    let code = if code.is_empty() {
        "network_error".into()
    } else {
        code
    };
    let message = format!("Host HTTP broker: {code}");
    PluginError::new(code, message)
}

/// Decode a provider body. A document that stops early (cut connection, empty 200) is
/// `truncated_response`; any other defect is `parse_error`.
pub fn parse_body(body: &[u8]) -> Result<serde_json::Value, PluginError> {
    serde_json::from_slice(body).map_err(|e| json_error(&e))
}

/// Typed form of a body decoding failure (see [`parse_body`]), for plugins that decode
/// straight into their own structures.
pub fn json_error(error: &serde_json::Error) -> PluginError {
    if error.is_eof() {
        PluginError::new("truncated_response", "The provider response ended early")
    } else {
        PluginError::new("parse_error", error.to_string())
    }
}

/// Native transport failure without the request URL (it may carry an API key).
#[cfg(not(target_arch = "wasm32"))]
pub fn transport_error(error: reqwest::Error) -> PluginError {
    let error = error.without_url();
    if error.is_timeout() {
        PluginError::new("timeout", "The provider did not answer in time")
    } else {
        PluginError::new("network_error", "The provider could not be reached")
    }
}

/// Read a native response body up to [`MAX_RESPONSE_BYTES`].
#[cfg(not(target_arch = "wasm32"))]
pub async fn read_body(mut response: reqwest::Response) -> Result<Vec<u8>, PluginError> {
    let mut body = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    return Err(PluginError::new(
                        "response_too_large",
                        "The provider response exceeds the size limit",
                    ));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(body),
            Err(e) if e.is_timeout() => {
                return Err(PluginError::new(
                    "timeout",
                    "The provider stopped sending data",
                ));
            }
            Err(_) => {
                return Err(PluginError::new(
                    "truncated_response",
                    "The connection ended before the whole response arrived",
                ));
            }
        }
    }
}

/// Minimum spacing between requests to a rate-limited provider (native execution only).
///
/// Reserves the next free slot for key and waits for it, so concurrent callers are spaced out.
/// On wasm32 guests there is no clock or timer: the Aether host broker must enforce provider
/// limits there, and a provider answering 429 is mapped to `rate_limited`. The SDK never sleeps
/// or retries on its own account: the Core turns a 429/503 into a durable cooldown and the
/// caller decides when to ask again.
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
        let resp = crate::host_http_fetch(&request).map_err(host_error)?;
        if let Some(err) = classify_status(
            resp.status,
            retry_after_from_headers(&resp.headers),
            provider,
            what,
        ) {
            return Err(err);
        }
        parse_body(&resp.body)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let timeout = std::time::Duration::from_secs(config.timeout_secs.unwrap_or(10));
        // No redirect: the Core broker refuses them, and following one would resend the
        // caller's headers (credentials included) to whatever host the provider names.
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| PluginError::new("client_error", e.without_url().to_string()))?;
        let mut req = client.get(url).header("Accept", "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let res = req.send().await.map_err(transport_error)?;
        let retry_after = retry_after_secs(
            res.headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok()),
        );
        if let Some(err) = classify_status(res.status().as_u16(), retry_after, provider, what) {
            return Err(err);
        }
        parse_body(&read_body(res).await?)
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

    #[test]
    fn every_failure_class_has_its_own_code() {
        for (status, code) in [
            (401, "unauthorized"),
            (403, "forbidden"),
            (404, "not_found"),
            (410, "not_found"),
            (408, "timeout"),
            (429, "rate_limited"),
            (502, "provider_unavailable"),
            (503, "provider_unavailable"),
            (504, "provider_unavailable"),
            (302, "provider_redirect_refused"),
            (400, "provider_error"),
            (500, "provider_error"),
            (501, "provider_error"),
        ] {
            let error = classify_status(status, None, "P", "x").unwrap();
            assert_eq!(error.code, code, "HTTP {status}");
            assert!(!error.message.contains("http"), "HTTP {status}");
        }
        assert!(classify_status(204, None, "P", "x").is_none());
    }

    #[test]
    fn retry_after_is_honoured_not_invented() {
        assert_eq!(
            classify_status(429, Some(7), "P", "x")
                .unwrap()
                .retry_after_secs,
            Some(7)
        );
        // No usable header: the historical hint on 429, nothing on an outage.
        assert_eq!(
            classify_status(429, None, "P", "x")
                .unwrap()
                .retry_after_secs,
            Some(10)
        );
        assert_eq!(
            classify_status(503, None, "P", "x")
                .unwrap()
                .retry_after_secs,
            None
        );
        assert_eq!(
            classify_status(503, Some(30), "P", "x")
                .unwrap()
                .retry_after_secs,
            Some(30)
        );
        assert_eq!(retry_after_secs(Some(" 120 ")), Some(120));
        assert_eq!(retry_after_secs(Some("0")), Some(1));
        assert_eq!(retry_after_secs(Some("999999")), Some(86_400));
        // HTTP-date and garbage need a clock: left to the Core broker.
        assert_eq!(
            retry_after_secs(Some("Wed, 21 Oct 2026 07:28:00 GMT")),
            None
        );
        assert_eq!(retry_after_secs(Some("-5")), None);
        assert_eq!(retry_after_secs(Some("")), None);
        assert_eq!(retry_after_secs(None), None);
        let headers =
            std::collections::BTreeMap::from([("Retry-After".to_string(), "9".to_string())]);
        assert_eq!(retry_after_from_headers(&headers), Some(9));
    }

    #[test]
    fn broker_errors_keep_their_code_and_lose_the_url() {
        let leaked = crate::HostHttpError {
            code: "network_error".into(),
            message: "error sending request for url (https://api.example.org/3/movie/1?api_key=SECRET&x=1)".into(),
        };
        let error = host_error(leaked);
        assert_eq!(error.code, "network_error");
        assert!(!error.message.contains("SECRET"), "{}", error.message);
        assert!(!error.message.contains("example.org"), "{}", error.message);
        assert!(!error.message.contains("http"), "{}", error.message);
        for (broker, plugin) in [
            ("dns_timeout", "timeout"),
            ("provider_rate_limited", "rate_limited"),
            ("quota_exceeded", "quota_exceeded"),
            ("domain_not_allowed", "domain_not_allowed"),
            ("response_too_large", "response_too_large"),
            ("", "network_error"),
        ] {
            let error = host_error(crate::HostHttpError {
                code: broker.into(),
                message: "x".into(),
            });
            assert_eq!(error.code, plugin, "{broker}");
        }
        assert_eq!(
            host_error(crate::HostHttpError {
                code: "a b/../c".into(),
                message: "x".into()
            })
            .code,
            "abc"
        );
    }

    #[test]
    fn truncated_documents_are_told_apart_from_malformed_ones() {
        assert_eq!(
            parse_body(br#"{"a":[1,2"#).unwrap_err().code,
            "truncated_response"
        );
        assert_eq!(parse_body(b"").unwrap_err().code, "truncated_response");
        assert_eq!(parse_body(b"<html>").unwrap_err().code, "parse_error");
        assert_eq!(parse_body(br#"{"a":1}"#).unwrap()["a"], 1);
    }
}
