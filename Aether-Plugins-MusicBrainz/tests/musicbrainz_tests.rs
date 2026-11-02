use aether_plugin_api::{
    DetailsRequest, ImagesRequest, MediaType, MetadataProviderPlugin, ProviderConfig, SearchQuery,
};
use aether_plugin_musicbrainz::MusicBrainzPlugin;
use http_body_util::Full;
use hyper::server::conn::http1;
use hyper::{Request, Response, body::Bytes, service::service_fn};
use hyper_util::rt::TokioIo;
use std::{convert::Infallible, net::SocketAddr};
use tokio::net::TcpListener;

async fn serve(handler: fn(&str) -> (u16, String)) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .serve_connection(
                        io,
                        service_fn(move |req: Request<hyper::body::Incoming>| async move {
                            let target = req
                                .uri()
                                .path_and_query()
                                .map(|p| p.as_str().to_string())
                                .unwrap_or_default();
                            let (status, body) = handler(&target);
                            Ok::<_, Infallible>(
                                Response::builder()
                                    .status(status)
                                    .header("Content-Type", "application/json")
                                    .body(Full::new(Bytes::from(body)))
                                    .unwrap(),
                            )
                        }),
                    )
                    .await;
            });
        }
    });
    addr
}

fn mock(target: &str) -> (u16, String) {
    if target.starts_with("/release?query=") {
        return (
            200,
            r#"{"releases":[{"id":"11111111-2222-3333-4444-555555555555","title":"OK Computer","date":"1997-05-21","score":100,
               "artist-credit":[{"name":"Radiohead"}]}]}"#
                .into(),
        );
    }
    if target.starts_with("/release/11111111-2222-3333-4444-555555555555?") {
        return (
            200,
            r#"{"id":"11111111-2222-3333-4444-555555555555","title":"OK Computer","date":"1997-05-21","status":"Official",
               "artist-credit":[{"name":"Radiohead"}],"release-group":{"id":"aaaa-bbbb"},
               "genres":[{"name":"alternative rock"}],
               "media":[{"tracks":[{"length":120000},{"length":180000},{"length":null}]}]}"#
                .into(),
        );
    }
    if target == "/cover/release/11111111-2222-3333-4444-555555555555" {
        return (
            200,
            r#"{"images":[{"image":"http://coverartarchive.org/release/x/back.jpg","front":false,"back":true},
               {"image":"http://coverartarchive.org/release/x/front.jpg","front":true,"back":false}]}"#
                .into(),
        );
    }
    (404, "{}".into())
}

fn config(addr: SocketAddr) -> ProviderConfig {
    let mut cfg = ProviderConfig {
        base_url: Some(format!("http://{addr}")),
        ..Default::default()
    };
    cfg.extra
        .insert("cover_art_url".into(), format!("http://{addr}/cover"));
    cfg.extra.insert("min_interval_ms".into(), "0".into());
    cfg
}

const MBID: &str = "11111111-2222-3333-4444-555555555555";

#[tokio::test]
async fn search_returns_releases_with_artist() {
    let addr = serve(mock).await;
    let items = MusicBrainzPlugin::new()
        .search(
            SearchQuery {
                title: "OK \"Computer\"".into(),
                year: None,
                media_type: MediaType::Movie,
                language: None,
                external_ids: [("artist".to_string(), "Radiohead".to_string())].into(),
            },
            &config(addr),
        )
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, MBID);
    assert_eq!(items[0].year, Some(1997));
    assert_eq!(items[0].overview.as_deref(), Some("Radiohead"));
}

#[tokio::test]
async fn details_aggregate_tracks_and_ids() {
    let addr = serve(mock).await;
    let d = MusicBrainzPlugin::new()
        .details(
            DetailsRequest {
                id: MBID.into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config(addr),
        )
        .await
        .unwrap();
    assert_eq!(d.title.as_deref(), Some("OK Computer"));
    assert_eq!(d.runtime_minutes, Some(5));
    assert_eq!(d.genres, vec!["alternative rock"]);
    assert_eq!(d.actors[0].name, "Radiohead");
    assert_eq!(d.external_ids["musicbrainz_release_group"], "aaaa-bbbb");
}

#[tokio::test]
async fn cover_art_marks_front_primary_and_upgrades_to_https() {
    let addr = serve(mock).await;
    let imgs = MusicBrainzPlugin::new()
        .images(
            ImagesRequest {
                id: MBID.into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config(addr),
        )
        .await
        .unwrap();
    assert_eq!(imgs.len(), 2);
    assert!(!imgs[0].is_primary);
    assert!(imgs[1].is_primary);
    assert!(imgs[1].url.starts_with("https://"));
}

#[tokio::test]
async fn invalid_ids_are_rejected_before_any_request() {
    let plugin = MusicBrainzPlugin::new();
    let err = plugin
        .details(
            DetailsRequest {
                id: "../../etc".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &ProviderConfig::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalid_id");
}

#[tokio::test]
async fn missing_release_is_not_found() {
    let addr = serve(mock).await;
    let err = MusicBrainzPlugin::new()
        .details(
            DetailsRequest {
                id: "deadbeef-0000".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config(addr),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "not_found");
}

#[tokio::test]
async fn requests_are_spaced_by_default() {
    let addr = serve(mock).await;
    let cfg = ProviderConfig {
        base_url: Some(format!("http://{addr}")),
        ..Default::default()
    };
    let plugin = MusicBrainzPlugin::new();
    let query = || SearchQuery {
        title: "OK Computer".into(),
        year: None,
        media_type: MediaType::Music,
        language: None,
        external_ids: Default::default(),
    };
    let start = std::time::Instant::now();
    plugin.search(query(), &cfg).await.unwrap();
    plugin.search(query(), &cfg).await.unwrap();
    assert!(start.elapsed() >= std::time::Duration::from_millis(1000));
}

#[tokio::test]
async fn extension_search_returns_structured_music_artist() {
    let addr = serve(mock).await;
    let query = SearchQuery {
        title: "OK Computer".into(),
        year: None,
        media_type: MediaType::Music,
        language: None,
        external_ids: [("artist".to_string(), "Radiohead".to_string())].into(),
    };
    let results = MusicBrainzPlugin::new()
        .search(query, &config(addr))
        .await
        .unwrap();
    let mut response = aether_plugin_api::ExtensionResponse {
        api_version: 1,
        result: serde_json::to_value(results).unwrap(),
    };
    aether_plugin_api::http::promote_music_artist(&mut response);
    assert_eq!(response.result[0]["artist"], "Radiohead");
    assert_eq!(response.result[0]["title"], "OK Computer");
    assert_eq!(response.result[0]["mediaType"], "music");
}
