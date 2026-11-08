//! Simulated provider failures. A scripted server answers every request and counts them, so
//! each test shows both the error class the caller sees and that nothing was retried.

use aether_plugin_api::{
    DetailsRequest, ImagesRequest, MediaType, MetadataProviderPlugin, PluginError, ProviderConfig,
    StructuredMetadata,
};
use aether_plugin_tmdb::TmdbPlugin;
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
    paths: Arc<Mutex<Vec<String>>>,
}

/// `route(path_and_query)` builds the reply to every request.
async fn serve(route: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = Server {
        url: format!("http://{}", listener.local_addr().unwrap()),
        hits: Arc::new(AtomicUsize::new(0)),
        paths: Arc::new(Mutex::new(Vec::new())),
    };
    let (hits, paths, route) = (server.hits.clone(), server.paths.clone(), Arc::new(route));
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            hits.fetch_add(1, Ordering::SeqCst);
            let (paths, route) = (paths.clone(), route.clone());
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
                paths.lock().unwrap().push(target.clone());
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

fn config(url: &str) -> ProviderConfig {
    ProviderConfig {
        api_key: Some(KEY.into()),
        base_url: Some(url.into()),
        timeout_secs: Some(1),
        ..Default::default()
    }
}

async fn details(url: &str) -> Result<StructuredMetadata, PluginError> {
    TmdbPlugin::new()
        .details(
            DetailsRequest {
                id: "42".into(),
                media_type: MediaType::Movie,
                ..Default::default()
            },
            &config(url),
        )
        .await
}

#[tokio::test]
async fn every_failure_class_has_a_distinct_code_and_costs_one_request() {
    for (status, headers, code, retry) in [
        ("401 Unauthorized", vec![], "unauthorized", None),
        ("403 Forbidden", vec![], "forbidden", None),
        ("404 Not Found", vec![], "not_found", None),
        (
            "429 Too Many Requests",
            vec!["Retry-After: 7"],
            "rate_limited",
            Some(7),
        ),
        ("500 Internal Server Error", vec![], "provider_error", None),
        (
            "503 Service Unavailable",
            vec![],
            "provider_unavailable",
            None,
        ),
    ] {
        let headers = headers.clone();
        let server = serve(move |_| http(status, &headers, b"{}")).await;
        let error = details(&server.url).await.unwrap_err();
        assert_eq!(error.code, code, "{status}");
        if retry.is_some() {
            assert_eq!(error.retry_after_secs, retry, "{status}");
        }
        assert!(!error.message.contains(KEY), "{status}: {}", error.message);
        assert_eq!(server.hits.load(Ordering::SeqCst), 1, "{status}: retried");
    }
}

#[tokio::test]
async fn silence_truncation_and_dead_hosts_are_reported_without_the_key() {
    let server = serve(|_| None).await;
    assert_eq!(details(&server.url).await.unwrap_err().code, "timeout");
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);

    let server = serve(|_| http("200 OK", &["Content-Length: 500"], br#"{"id":42,"ti"#)).await;
    assert_eq!(
        details(&server.url).await.unwrap_err().code,
        "truncated_response"
    );

    let server = serve(|_| http("200 OK", &[], br#"{"id":42,"title":"Cut"#)).await;
    assert_eq!(
        details(&server.url).await.unwrap_err().code,
        "truncated_response"
    );

    let dead = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let error = details(&dead).await.unwrap_err();
    assert_eq!(error.code, "network_error");
    assert!(!error.message.contains(KEY), "leaked: {}", error.message);
}

#[tokio::test]
async fn a_missing_title_is_not_found_not_a_provider_fault() {
    let server = serve(|_| http("404 Not Found", &[], br#"{"status_code":34}"#)).await;
    let error = TmdbPlugin::new()
        .images(
            ImagesRequest {
                id: "42".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config(&server.url),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "not_found");
}

#[tokio::test]
async fn a_blank_imdb_id_does_not_hide_a_valid_tvdb_id() {
    let server = serve(|target| {
        if target.starts_with("/find/987") && target.contains("external_source=tvdb_id") {
            http(
                "200 OK",
                &[],
                br#"{"movie_results":[{"id":42}],"tv_results":[]}"#,
            )
        } else if target.starts_with("/movie/42") {
            http("200 OK", &[], br#"{"id":42,"title":"Resolved"}"#)
        } else {
            http("404 Not Found", &[], b"{}")
        }
    })
    .await;
    let meta = TmdbPlugin::new()
        .details(
            DetailsRequest {
                media_type: MediaType::Movie,
                external_ids: [("imdb".into(), "  ".into()), ("tvdb".into(), "987".into())].into(),
                ..Default::default()
            },
            &config(&server.url),
        )
        .await
        .unwrap();
    assert_eq!(meta.provider_id, "42");
}

#[tokio::test]
async fn an_external_id_matching_several_works_is_ambiguous() {
    let server = serve(|target| {
        if target.starts_with("/find/tt0000042") {
            http("200 OK", &[], br#"{"movie_results":[{"id":42},{"id":43}]}"#)
        } else {
            http("500 Internal Server Error", &[], b"{}")
        }
    })
    .await;
    let error = TmdbPlugin::new()
        .details(
            DetailsRequest {
                id: "tt0000042".into(),
                media_type: MediaType::Movie,
                ..Default::default()
            },
            &config(&server.url),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "ambiguous_match");
    assert_eq!(
        server.hits.load(Ordering::SeqCst),
        1,
        "no guess among candidates"
    );
    assert!(
        server
            .paths
            .lock()
            .unwrap()
            .iter()
            .all(|p| !p.starts_with("/movie/"))
    );
}
