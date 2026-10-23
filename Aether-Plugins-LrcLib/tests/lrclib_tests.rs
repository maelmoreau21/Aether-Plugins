use aether_plugin_api::{
    DetailsRequest, LyricsRequest, MediaType, MetadataProviderPlugin, ProviderConfig, SearchQuery,
};
use aether_plugin_lrclib::{LrcLibPlugin, handle_lyrics_request, parse_lrc};
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

const TRACK: &str = r#"{"id":3396226,"trackName":"I Want to Live","artistName":"Borislav Slavov","albumName":"Baldur's Gate 3",
  "duration":233.0,"instrumental":false,"plainLyrics":"I feel your breath","syncedLyrics":"[00:17.12] I feel your breath\n[00:20.5] on my skin"}"#;

fn mock(target: &str) -> (u16, String) {
    if target.starts_with("/get?") && target.contains("track_name=I%20Want%20to%20Live") {
        return (200, TRACK.into());
    }
    if target == "/get/3396226" {
        return (200, TRACK.into());
    }
    if target.starts_with("/search?") {
        return (200, format!("[{TRACK}]"));
    }
    (404, r#"{"code":404,"name":"TrackNotFound"}"#.into())
}

fn config(addr: SocketAddr) -> ProviderConfig {
    ProviderConfig {
        base_url: Some(format!("http://{addr}")),
        ..Default::default()
    }
}

#[test]
fn lrc_parser_handles_formats_multi_stamps_and_metadata() {
    let lines =
        parse_lrc("[ar:Someone]\n[00:12.5] b\n[00:01.00][00:30] a\n[01:02.123]c\nplain\n[xx:yy]z");
    let got: Vec<_> = lines.iter().map(|l| (l.time_ms, l.text.as_str())).collect();
    assert_eq!(
        got,
        vec![(1000, "a"), (12500, "b"), (30000, "a"), (62123, "c")]
    );
}

#[tokio::test]
async fn get_lyrics_by_track_returns_plain_and_synced() {
    let addr = serve(mock).await;
    let l = LrcLibPlugin::new()
        .get_lyrics(
            LyricsRequest {
                track: "I Want to Live".into(),
                artist: "Borislav Slavov".into(),
                album: Some("Baldur's Gate 3".into()),
                duration_secs: Some(233),
                ..Default::default()
            },
            &config(addr),
        )
        .await
        .unwrap();
    assert_eq!(l.id, 3396226);
    assert_eq!(l.plain.as_deref(), Some("I feel your breath"));
    assert_eq!(l.synced_lines.len(), 2);
    assert_eq!(l.synced_lines[0].time_ms, 17120);
    assert_eq!(l.synced_lines[1].time_ms, 20500);
}

#[tokio::test]
async fn get_lyrics_falls_back_to_search_when_exact_match_is_missing() {
    let addr = serve(mock).await;
    let l = LrcLibPlugin::new()
        .get_lyrics(
            LyricsRequest {
                track: "Unknown spelling".into(),
                artist: "x".into(),
                ..Default::default()
            },
            &config(addr),
        )
        .await
        .unwrap();
    assert_eq!(l.track_name, "I Want to Live");
}

#[tokio::test]
async fn trait_search_and_details_use_ids() {
    let addr = serve(mock).await;
    let plugin = LrcLibPlugin::new();
    let items = plugin
        .search(
            SearchQuery {
                title: "I Want".into(),
                year: None,
                media_type: MediaType::Movie,
                language: None,
                external_ids: Default::default(),
            },
            &config(addr),
        )
        .await
        .unwrap();
    assert_eq!(items[0].id, "3396226");
    assert_eq!(
        items[0].overview.as_deref(),
        Some("Borislav Slavov â€” Baldur's Gate 3")
    );
    let d = plugin
        .details(
            DetailsRequest {
                id: items[0].id.clone(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config(addr),
        )
        .await
        .unwrap();
    assert_eq!(d.synopsis.as_deref(), Some("I feel your breath"));
    let err = plugin
        .details(
            DetailsRequest {
                id: "abc".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config(addr),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalid_id");
}

#[tokio::test]
async fn lyrics_get_operation_and_validation() {
    let addr = serve(mock).await;
    let plugin = LrcLibPlugin::new();
    let req = serde_json::json!({
        "apiVersion": 1, "operation": "lyrics.get",
        "input": {"id": 3396226}, "config": {"baseUrl": format!("http://{addr}")}
    });
    let out: serde_json::Value =
        serde_json::from_str(&handle_lyrics_request(&plugin, &req.to_string()).await).unwrap();
    assert_eq!(out["success"], true);
    assert_eq!(out["data"]["syncedLines"][0]["timeMs"], 17120);

    let bad = serde_json::json!({"apiVersion":1,"operation":"lyrics.get","input":{}});
    let out: serde_json::Value =
        serde_json::from_str(&handle_lyrics_request(&plugin, &bad.to_string()).await).unwrap();
    assert_eq!(out["error"]["code"], "missing_track");

    let manifest = serde_json::json!({"apiVersion":1,"operation":"manifest"});
    let out: serde_json::Value =
        serde_json::from_str(&handle_lyrics_request(&plugin, &manifest.to_string()).await).unwrap();
    assert_eq!(out["data"]["id"], "lrclib");
}
