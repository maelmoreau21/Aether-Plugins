//! TheTVDB v4 against a scripted server (native-only `base_url`): login, lookup, translations,
//! and every failure class. Each case counts the requests so a hidden retry would show.

use aether_plugin_api::{
    DetailsRequest, MediaType, MetadataProviderPlugin, PluginError, ProviderConfig, SearchQuery,
    StructuredMetadata,
};
use aether_plugin_tvdb::TvdbPlugin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const KEY: &str = "SECRET-KEY";

/// `Some(bytes)` answers, `None` accepts and stays silent.
type Reply = Option<Vec<u8>>;

fn http(status: &str, headers: &[&str], body: &str) -> Reply {
    let mut out = format!("HTTP/1.1 {status}\r\nConnection: close\r\n").into_bytes();
    for header in headers {
        out.extend_from_slice(format!("{header}\r\n").as_bytes());
    }
    if !headers.iter().any(|h| h.starts_with("Content-Length")) {
        out.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body.as_bytes());
    Some(out)
}

fn ok(data: &str) -> Reply {
    http(
        "200 OK",
        &[],
        &format!(r#"{{"status":"success","data":{data}}}"#),
    )
}

struct Server {
    url: String,
    hits: Arc<AtomicUsize>,
    paths: Arc<Mutex<Vec<String>>>,
}

/// `route(method, path_and_query)` builds the reply to every request.
async fn serve(route: impl Fn(&str, &str) -> Reply + Send + Sync + 'static) -> Server {
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
                // Read the head, then the announced body, so closing never resets the client.
                let mut seen = Vec::new();
                let mut buf = [0u8; 2048];
                let head_end = loop {
                    if let Some(at) = seen.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => seen.extend_from_slice(&buf[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&seen[..head_end]).into_owned();
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while seen.len() < head_end + length {
                    match stream.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => seen.extend_from_slice(&buf[..n]),
                    }
                }
                let mut parts = head.split_whitespace();
                let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
                paths.lock().unwrap().push(format!("{method} {target}"));
                match route(method, target) {
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
        language: Some("fr-FR".into()),
        timeout_secs: Some(1),
        extra: [("access_model".to_string(), "licensed".to_string())].into(),
        ..Default::default()
    }
}

fn movie(ids: &[(&str, &str)], languages: &[&str]) -> DetailsRequest {
    DetailsRequest {
        media_type: MediaType::Movie,
        external_ids: ids
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        languages: languages.iter().map(|l| l.to_string()).collect(),
        ..Default::default()
    }
}

async fn details(url: &str) -> Result<StructuredMetadata, PluginError> {
    TvdbPlugin
        .details(movie(&[("tvdb", "42")], &[]), &config(url))
        .await
}

/// A healthy catalogue: login, one movie, French translation, no English one.
fn catalogue(method: &str, target: &str) -> Reply {
    match (method, target) {
        ("POST", "/login") => ok(r#"{"token":"tok"}"#),
        ("GET", "/search/remoteid/tt0000042") => ok(r#"[{"movie":{"id":42}}]"#),
        ("GET", "/search/remoteid/tt0000043") => ok(r#"[{"movie":{"id":42}},{"movie":{"id":43}}]"#),
        ("GET", "/movies/42/extended") => {
            ok(r#"{"id":42,"name":"Film","remoteIds":[{"sourceName":"IMDB","id":"tt0000042"}]}"#)
        }
        ("GET", "/movies/42/translations/fra") => ok(r#"{"name":"Titre","overview":"Résumé"}"#),
        ("GET", "/movies/42/translations/eng") => http("404 Not Found", &[], "{}"),
        ("GET", t) if t.starts_with("/search?") => {
            ok(r#"[{"type":"movie","tvdb_id":"42","name":"Film","year":"1999"}]"#)
        }
        _ => http("500 Internal Server Error", &[], "{}"),
    }
}

#[tokio::test]
async fn the_healthy_catalogue_resolves_and_translates() {
    let server = serve(catalogue).await;
    let meta = details(&server.url).await.unwrap();
    assert_eq!(meta.provider_id, "42");
    assert_eq!(meta.title.as_deref(), Some("Titre"));
    assert_eq!(meta.synopsis.as_deref(), Some("Résumé"));
}

#[tokio::test]
async fn every_failure_class_has_a_distinct_code_and_costs_one_request() {
    for (status, headers, code, retry) in [
        ("401 Unauthorized", vec![], "unauthorized", None),
        ("403 Forbidden", vec![], "forbidden", None),
        (
            "429 Too Many Requests",
            vec!["Retry-After: 7"],
            "rate_limited",
            Some(7),
        ),
        ("500 Internal Server Error", vec![], "provider_error", None),
        (
            "503 Service Unavailable",
            vec!["Retry-After: 4"],
            "provider_unavailable",
            Some(4),
        ),
        ("504 Gateway Timeout", vec![], "provider_unavailable", None),
        (
            "302 Found",
            vec!["Location: https://elsewhere.invalid/"],
            "provider_redirect_refused",
            None,
        ),
    ] {
        let headers = headers.clone();
        let server = serve(move |_, _| http(status, &headers, "{}")).await;
        let error = details(&server.url).await.unwrap_err();
        assert_eq!(error.code, code, "{status}");
        assert_eq!(error.retry_after_secs, retry, "{status}");
        assert!(!error.message.contains(KEY), "{status}: {}", error.message);
        assert_eq!(
            server.hits.load(Ordering::SeqCst),
            1,
            "{status}: login retried"
        );
    }
}

#[tokio::test]
async fn a_missing_record_is_not_found() {
    let server = serve(|method, target| match (method, target) {
        ("POST", "/login") => ok(r#"{"token":"tok"}"#),
        _ => http("404 Not Found", &[], r#"{"status":"failure"}"#),
    })
    .await;
    assert_eq!(details(&server.url).await.unwrap_err().code, "not_found");
    assert_eq!(
        server.hits.load(Ordering::SeqCst),
        2,
        "login then one lookup"
    );
}

#[tokio::test]
async fn silence_truncation_and_dead_hosts_are_reported_without_the_key() {
    let server = serve(|_, _| None).await;
    assert_eq!(details(&server.url).await.unwrap_err().code, "timeout");
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);

    let server = serve(|_, _| http("200 OK", &["Content-Length: 500"], r#"{"status":"su"#)).await;
    assert_eq!(
        details(&server.url).await.unwrap_err().code,
        "truncated_response"
    );

    let server = serve(|_, _| http("200 OK", &[], r#"{"status":"success","data":{"tok"#)).await;
    assert_eq!(
        details(&server.url).await.unwrap_err().code,
        "truncated_response"
    );

    let server = serve(|_, _| http("200 OK", &[], "<html>maintenance</html>")).await;
    assert_eq!(
        details(&server.url).await.unwrap_err().code,
        "invalid_provider_response"
    );

    let server = serve(|_, _| http("200 OK", &[], &" ".repeat(300_000))).await;
    assert_eq!(
        details(&server.url).await.unwrap_err().code,
        "provider_response_too_large"
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
async fn a_blank_tvdb_id_does_not_hide_a_usable_imdb_id() {
    let server = serve(catalogue).await;
    let meta = TvdbPlugin
        .details(
            movie(&[("tvdb", " "), ("imdb", "tt0000042")], &[]),
            &config(&server.url),
        )
        .await
        .unwrap();
    assert_eq!(meta.provider_id, "42");
    // A blank imdb entry must not poison the identity check of a good tvdb id either.
    let meta = TvdbPlugin
        .details(
            movie(&[("tvdb", "42"), ("imdb", "")], &[]),
            &config(&server.url),
        )
        .await
        .unwrap();
    assert_eq!(meta.provider_id, "42");
}

#[tokio::test]
async fn blank_identifiers_fall_back_to_the_title_search() {
    let server = serve(catalogue).await;
    let results = TvdbPlugin
        .search(
            SearchQuery {
                title: "Film".into(),
                year: None,
                media_type: MediaType::Movie,
                language: None,
                external_ids: [("tvdb".into(), " ".into()), ("imdb".into(), "".into())].into(),
            },
            &config(&server.url),
        )
        .await
        .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "42");
}

#[tokio::test]
async fn an_imdb_id_matching_several_records_is_ambiguous() {
    let server = serve(catalogue).await;
    let error = TvdbPlugin
        .details(movie(&[("imdb", "tt0000043")], &[]), &config(&server.url))
        .await
        .unwrap_err();
    assert_eq!(error.code, "ambiguous_match");
    assert!(
        server
            .paths
            .lock()
            .unwrap()
            .iter()
            .all(|p| !p.contains("/movies/"))
    );
}

#[tokio::test]
async fn a_failing_extra_language_keeps_the_record_but_a_credential_failure_does_not() {
    let flaky = |status: &'static str| {
        move |method: &str, target: &str| match (method, target) {
            ("GET", "/movies/42/translations/deu") => http(status, &[], "{}"),
            _ => catalogue(method, target),
        }
    };
    let server = serve(flaky("500 Internal Server Error")).await;
    let meta = TvdbPlugin
        .details(movie(&[("tvdb", "42")], &["de"]), &config(&server.url))
        .await
        .unwrap();
    assert_eq!(meta.title.as_deref(), Some("Titre"));
    assert!(meta.translations.contains_key("fra"));
    assert!(!meta.translations.contains_key("deu"));

    let server = serve(flaky("403 Forbidden")).await;
    let error = TvdbPlugin
        .details(movie(&[("tvdb", "42")], &["de"]), &config(&server.url))
        .await
        .unwrap_err();
    assert_eq!(error.code, "forbidden");
}
