//! Native `get_json` against a scripted HTTP server: every failure class, one request each.

use aether_plugin_api::ProviderConfig;
use aether_plugin_api::http::get_json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

enum Reply {
    Send(Vec<u8>),
    /// Accept, read the request, never answer.
    Hang,
}

fn response(status: &str, headers: &[&str], body: &[u8]) -> Reply {
    let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n").into_bytes();
    for header in headers {
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    if !headers.iter().any(|h| h.starts_with("Content-Length")) {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    Reply::Send(out)
}

/// Serve `reply(n)` to the n-th connection; returns the base URL and the request counter.
async fn serve(
    reply: impl Fn(usize) -> Reply + Send + Sync + 'static,
) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let reply = Arc::new(reply);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let reply = reply.clone();
            tokio::spawn(async move {
                let mut seen = Vec::new();
                let mut buf = [0u8; 1024];
                while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(read) => seen.extend_from_slice(&buf[..read]),
                    }
                }
                match reply(n) {
                    Reply::Send(bytes) => {
                        let _ = stream.write_all(&bytes).await;
                        let _ = stream.shutdown().await;
                    }
                    Reply::Hang => tokio::time::sleep(std::time::Duration::from_secs(30)).await,
                }
            });
        }
    });
    (url, hits)
}

fn config(timeout: u64) -> ProviderConfig {
    ProviderConfig {
        timeout_secs: Some(timeout),
        ..Default::default()
    }
}

async fn fetch(url: &str) -> Result<serde_json::Value, aether_plugin_api::PluginError> {
    get_json(
        &format!("{url}/x?api_key=SECRET"),
        &[],
        &config(5),
        "Provider",
        "item",
    )
    .await
}

#[tokio::test]
async fn success_returns_the_document() {
    let (url, hits) = serve(|_| response("200 OK", &[], br#"{"ok":true}"#)).await;
    assert_eq!(fetch(&url).await.unwrap()["ok"], true);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn failures_are_classified_and_never_retried() {
    for (status, headers, code, retry) in [
        ("401 Unauthorized", vec![], "unauthorized", None),
        ("403 Forbidden", vec![], "forbidden", None),
        ("404 Not Found", vec![], "not_found", None),
        ("408 Request Timeout", vec![], "timeout", None),
        (
            "429 Too Many Requests",
            vec!["Retry-After: 7"],
            "rate_limited",
            Some(7),
        ),
        ("429 Too Many Requests", vec![], "rate_limited", Some(10)),
        (
            "429 Too Many Requests",
            vec!["Retry-After: Wed, 21 Oct 2026 07:28:00 GMT"],
            "rate_limited",
            Some(10),
        ),
        (
            "503 Service Unavailable",
            vec!["Retry-After: 3"],
            "provider_unavailable",
            Some(3),
        ),
        (
            "503 Service Unavailable",
            vec![],
            "provider_unavailable",
            None,
        ),
        ("504 Gateway Timeout", vec![], "provider_unavailable", None),
        ("500 Internal Server Error", vec![], "provider_error", None),
    ] {
        let headers = headers.clone();
        let (url, hits) = serve(move |_| response(status, &headers, b"{}")).await;
        let error = fetch(&url).await.unwrap_err();
        assert_eq!(error.code, code, "{status}");
        assert_eq!(error.retry_after_secs, retry, "{status}");
        assert!(
            !error.message.contains("SECRET"),
            "{status}: {}",
            error.message
        );
        assert_eq!(hits.load(Ordering::SeqCst), 1, "{status}: no hidden retry");
    }
}

#[tokio::test]
async fn redirects_are_refused_not_followed() {
    let (target, target_hits) = serve(|_| response("200 OK", &[], b"{}")).await;
    let location = format!("Location: {target}/elsewhere");
    let (url, hits) = serve(move |_| response("302 Found", &[&location], b"")).await;
    let error = fetch(&url).await.unwrap_err();
    assert_eq!(error.code, "provider_redirect_refused");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(target_hits.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cut_connections_and_cut_documents_are_truncated_responses() {
    // Announces 100 bytes, delivers 6, closes.
    let (url, _) = serve(|_| response("200 OK", &["Content-Length: 100"], br#"{"a":["#)).await;
    assert_eq!(fetch(&url).await.unwrap_err().code, "truncated_response");
    // Consistent length, but the JSON itself stops early.
    let (url, _) = serve(|_| response("200 OK", &[], br#"{"a":[1,2"#)).await;
    assert_eq!(fetch(&url).await.unwrap_err().code, "truncated_response");
    // Complete but not JSON.
    let (url, _) = serve(|_| response("200 OK", &[], b"<html>maintenance</html>")).await;
    assert_eq!(fetch(&url).await.unwrap_err().code, "parse_error");
}

#[tokio::test]
async fn silence_is_a_timeout() {
    let (url, hits) = serve(|_| Reply::Hang).await;
    let started = std::time::Instant::now();
    let error = get_json(&format!("{url}/x"), &[], &config(1), "Provider", "item")
        .await
        .unwrap_err();
    assert_eq!(error.code, "timeout");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn oversized_bodies_are_refused() {
    let body = vec![b' '; aether_plugin_api::http::MAX_RESPONSE_BYTES + 1];
    let (url, _) = serve(move |_| response("200 OK", &[], &body)).await;
    assert_eq!(fetch(&url).await.unwrap_err().code, "response_too_large");
}

#[tokio::test]
async fn unreachable_hosts_never_echo_the_request_url() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let error = fetch(&url).await.unwrap_err();
    assert_eq!(error.code, "network_error");
    assert!(!error.message.contains("SECRET"), "{}", error.message);
    assert!(!error.message.contains("127.0.0.1"), "{}", error.message);
}
