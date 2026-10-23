use aether_plugin_api::{
    DetailsRequest, ImagesRequest, MediaType, MetadataProviderPlugin, ProviderConfig, SearchQuery,
};
use aether_plugin_discogs::DiscogsPlugin;
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
    if target.starts_with("/database/search?") {
        return (
            200,
            r#"{"results":[{"id":249504,"title":"Radiohead - OK Computer","year":"1997","cover_image":"https://img.example/c.jpg","thumb":""}]}"#.into(),
        );
    }
    if target == "/releases/249504" {
        return (
            200,
            r#"{"id":249504,"title":"OK Computer","year":1997,"released":"1997-05-21","status":"Accepted",
               "notes":"Recorded in Oxfordshire.","artists":[{"name":"Radiohead"}],"genres":["Electronic","Rock"],"styles":["Alternative Rock"],
               "images":[{"type":"secondary","uri":"https://img.example/b.jpg","width":600,"height":600},
                         {"type":"primary","uri":"https://img.example/a.jpg","width":600,"height":601}],
               "community":{"rating":{"average":4.5,"count":1000}}}"#.into(),
        );
    }
    (404, "{}".into())
}

fn config(addr: SocketAddr, token: Option<&str>) -> ProviderConfig {
    ProviderConfig {
        api_key: token.map(String::from),
        base_url: Some(format!("http://{addr}")),
        ..Default::default()
    }
}

fn details_request(id: &str) -> DetailsRequest {
    DetailsRequest {
        id: id.into(),
        media_type: MediaType::Movie,
        external_ids: Default::default(),
    }
}

#[tokio::test]
async fn search_requires_token_and_splits_artist() {
    let addr = serve(mock).await;
    let plugin = DiscogsPlugin::new();
    let query = || SearchQuery {
        title: "OK Computer".into(),
        year: Some(1997),
        media_type: MediaType::Movie,
        language: None,
        external_ids: Default::default(),
    };
    let err = plugin
        .search(query(), &config(addr, None))
        .await
        .unwrap_err();
    assert_eq!(err.code, "missing_api_key");
    let items = plugin
        .search(query(), &config(addr, Some("tok")))
        .await
        .unwrap();
    assert_eq!(items[0].id, "249504");
    assert_eq!(items[0].title, "OK Computer");
    assert_eq!(items[0].overview.as_deref(), Some("Radiohead"));
    assert_eq!(
        items[0].poster_url.as_deref(),
        Some("https://img.example/c.jpg")
    );
}

#[tokio::test]
async fn details_merge_genres_styles_and_rating() {
    let addr = serve(mock).await;
    let d = DiscogsPlugin::new()
        .details(details_request("249504"), &config(addr, Some("tok")))
        .await
        .unwrap();
    assert_eq!(d.year, Some(1997));
    assert_eq!(d.genres, vec!["Electronic", "Rock", "Alternative Rock"]);
    assert_eq!(d.ratings[0].max, 5.0);
    assert_eq!(d.ratings[0].votes, Some(1000));
    assert_eq!(d.actors[0].name, "Radiohead");
}

#[tokio::test]
async fn images_flag_primary() {
    let addr = serve(mock).await;
    let imgs = DiscogsPlugin::new()
        .images(
            ImagesRequest {
                id: "249504".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config(addr, Some("tok")),
        )
        .await
        .unwrap();
    assert_eq!(imgs.len(), 2);
    assert!(imgs[1].is_primary && !imgs[0].is_primary);
}

#[tokio::test]
async fn invalid_and_unknown_ids_are_typed_errors() {
    let addr = serve(mock).await;
    let plugin = DiscogsPlugin::new();
    let err = plugin
        .details(details_request("12/../3"), &config(addr, None))
        .await
        .unwrap_err();
    assert_eq!(err.code, "invalid_id");
    let err = plugin
        .details(details_request("999"), &config(addr, None))
        .await
        .unwrap_err();
    assert_eq!(err.code, "not_found");
}
