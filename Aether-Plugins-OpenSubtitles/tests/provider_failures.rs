//! Simulated provider answers. A scripted server replies to every request and counts them, so
//! each case shows the code the caller sees and that nothing was retried (a download consumes
//! allowance even when its answer is lost).
use aether_plugin_api::{ExtensionRequest, ProviderConfig};
use aether_plugin_opensubtitles::run;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const KEY: &str = "SECRET-KEY";

/// `Some(bytes)` answers, `None` accepts and stays silent.
type Reply = Option<Vec<u8>>;

fn http(status: &str, headers: &[&str], body: &[u8]) -> Reply {
    let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n").into_bytes();
    for header in headers {
        out.extend_from_slice(format!("{header}\r\n").as_bytes());
    }
    if !headers.iter().any(|h| h.starts_with("Content-Length")) {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    Some(out)
}

struct Server {
    url: String,
    hits: Arc<AtomicUsize>,
    targets: Arc<Mutex<Vec<String>>>,
}

async fn serve(route: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = Server {
        url: format!("http://{}", listener.local_addr().unwrap()),
        hits: Arc::new(AtomicUsize::new(0)),
        targets: Arc::new(Mutex::new(Vec::new())),
    };
    let (hits, targets, route) = (server.hits.clone(), server.targets.clone(), Arc::new(route));
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            hits.fetch_add(1, Ordering::SeqCst);
            let (targets, route) = (targets.clone(), route.clone());
            tokio::spawn(async move {
                let mut seen = Vec::new();
                let mut buf = [0u8; 2048];
                while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => seen.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&seen).into_owned();
                let target = head.split_whitespace().nth(1).unwrap_or("/").to_owned();
                targets.lock().unwrap().push(target.clone());
                match route(&target) {
                    Some(bytes) => {
                        let _ = stream.write_all(&bytes).await;
                        let _ = stream.shutdown().await;
                    }
                    None => tokio::time::sleep(std::time::Duration::from_secs(30)).await,
                }
            });
        }
    });
    server
}

async fn call(url: &str, operation: &str, input: Value) -> Value {
    let mut input = input;
    input["config"] = json!({"api_key":KEY,"baseUrl":url,"timeoutSecs":1});
    // The scripted server must be the one reached: the config is parsed like the Core's.
    assert_eq!(
        ProviderConfig::from_extension_input(&input)
            .base_url
            .as_deref(),
        Some(url)
    );
    run(ExtensionRequest {
        api_version: 1,
        operation: operation.into(),
        input,
    })
    .await
    .result
}

fn search() -> Value {
    json!({"mediaType":"movie","title":"A Film","language":"fr"})
}

fn download() -> Value {
    json!({"fileId":"42"})
}

#[tokio::test]
async fn every_status_has_a_stable_code_and_costs_one_request() {
    for (status, operation, input, code) in [
        (
            "401 Unauthorized",
            "subtitles.search",
            search(),
            "unauthorized",
        ),
        (
            "403 Forbidden",
            "subtitles.search",
            search(),
            "unauthorized",
        ),
        (
            "404 Not Found",
            "subtitles.download",
            download(),
            "not_found",
        ),
        (
            "400 Bad Request",
            "subtitles.search",
            search(),
            "invalid_request",
        ),
        (
            "406 Not Acceptable",
            "subtitles.download",
            download(),
            "download_quota_exceeded",
        ),
        (
            "429 Too Many Requests",
            "subtitles.search",
            search(),
            "rate_limited",
        ),
        (
            "429 Too Many Requests",
            "subtitles.download",
            download(),
            "rate_limited",
        ),
        (
            "502 Bad Gateway",
            "subtitles.search",
            search(),
            "provider_unavailable",
        ),
        (
            "500 Internal Server Error",
            "subtitles.search",
            search(),
            "provider_error",
        ),
        (
            "302 Found",
            "subtitles.download",
            download(),
            "provider_redirect_refused",
        ),
    ] {
        let server = serve(move |_| {
            http(
                status,
                &["Retry-After: 7", "Location: http://127.0.0.1:1/"],
                br#"{"message":"SECRET-KEY leaked"}"#,
            )
        })
        .await;
        let result = call(&server.url, operation, input).await;
        assert_eq!(result, json!({"error":code}), "{status}");
        assert_eq!(server.hits.load(Ordering::SeqCst), 1, "{status}: retried");
        assert!(!result.to_string().contains(KEY));
    }
}

#[tokio::test]
async fn damaged_answers_are_named_and_never_retried() {
    // Valid HTTP, not JSON.
    let server = serve(|_| http("200 OK", &[], b"<html>maintenance</html>")).await;
    assert_eq!(
        call(&server.url, "subtitles.search", search()).await,
        json!({"error":"invalid_provider_response"})
    );
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    // Empty 200.
    let server = serve(|_| http("200 OK", &[], b"")).await;
    assert_eq!(
        call(&server.url, "subtitles.search", search()).await,
        json!({"error":"truncated_response"})
    );
    // A document cut in the middle.
    let server =
        serve(|_| http("200 OK", &[], br#"{"data":[{"attributes":{"language":"fr""#)).await;
    assert_eq!(
        call(&server.url, "subtitles.search", search()).await,
        json!({"error":"truncated_response"})
    );
    // The connection drops before the announced length.
    let server = serve(|_| http("200 OK", &["Content-Length: 5000"], br#"{"data":"#)).await;
    assert_eq!(
        call(&server.url, "subtitles.search", search()).await,
        json!({"error":"truncated_response"})
    );
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    // JSON of another shape is not a result list.
    let server = serve(|_| http("200 OK", &[], br#"{"data":"nope"}"#)).await;
    assert_eq!(
        call(&server.url, "subtitles.search", search()).await,
        json!({"error":"invalid_provider_response"})
    );
    // A body past the 256 KiB the Core broker accepts.
    let big = vec![b' '; 300_000];
    let server = serve(move |_| http("200 OK", &[], &big)).await;
    assert_eq!(
        call(&server.url, "subtitles.search", search()).await,
        json!({"error":"provider_response_too_large"})
    );
}

#[tokio::test]
async fn silence_is_a_timeout_and_a_download_is_not_replayed() {
    let server = serve(|_| None).await;
    let started = std::time::Instant::now();
    let result = call(&server.url, "subtitles.download", download()).await;
    assert_eq!(result, json!({"error":"timeout"}));
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn search_request_is_canonical_and_download_asks_for_srt_once() {
    let server = serve(|target| {
        if target.starts_with("/subtitles") {
            http("200 OK", &[], br#"{"total_pages":1,"data":[{"attributes":{"language":"pt-BR","release":"R","moviehash_match":true,"files":[{"file_id":9}]}}]}"#)
        } else {
            http("200 OK", &[], br#"{"link":"https://dl.opensubtitles.com/x","remaining":3,"reset_time_utc":"2026-11-08T00:00:00Z","requests":1}"#)
        }
    })
    .await;
    let result = call(
        &server.url,
        "subtitles.search",
        json!({"mediaType":"movie","title":"ignored","language":"PT-br","movieHash":"8E245D9679D31E12","externalIds":{"tmdb":"603"}}),
    )
    .await;
    assert_eq!(result["items"][0]["id"], "9");
    assert_eq!(result["items"][0]["hashMatch"], true);
    let target = server.targets.lock().unwrap()[0].clone();
    // Parameters are sorted so the service never redirects to its canonical order.
    assert_eq!(
        target,
        "/subtitles?languages=pt-br&moviehash=8e245d9679d31e12&tmdb_id=603&type=movie"
    );
    let result = call(&server.url, "subtitles.download", download()).await;
    assert_eq!(result["remaining"], 3);
    assert_eq!(result["resetTimeUtc"], "2026-11-08T00:00:00Z");
    assert!(result.get("requests").is_none());
    assert_eq!(server.hits.load(Ordering::SeqCst), 2);
}
