use aether_plugin_api::{
    DetailsRequest, ImagesRequest, MediaType, MetadataProviderPlugin, ProviderConfig, SearchQuery,
};
use aether_plugin_omdb::OmdbPlugin;
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
    if !target.contains("apikey=synthetic") {
        return (
            200,
            r#"{"Response":"False","Error":"Invalid API key!"}"#.into(),
        );
    }
    if target.contains("&s=") {
        return (
            200,
            r#"{"Search":[{"Title":"Interstellar","Year":"2014","imdbID":"tt0816692","Type":"movie","Poster":"https://img.example/p.jpg"},
               {"Title":"Other","Year":"2011–2019","imdbID":"tt1","Type":"series","Poster":"N/A"}],"Response":"True"}"#
                .into(),
        );
    }
    if target.contains("i=tt0816692") {
        return (
            200,
            r#"{"Title":"Interstellar","Year":"2014","Rated":"PG-13","Released":"07 Nov 2014","Runtime":"169 min",
                "Genre":"Adventure, Drama, Sci-Fi","Director":"Christopher Nolan","Writer":"Jonathan Nolan, Christopher Nolan",
                "Actors":"Matthew McConaughey, Anne Hathaway","Plot":"A team travels through a wormhole.",
                "Poster":"https://img.example/p.jpg","Ratings":[{"Source":"Internet Movie Database","Value":"8.7/10"},
                {"Source":"Rotten Tomatoes","Value":"73%"}],"imdbRating":"8.7","imdbVotes":"2,100,000",
                "imdbID":"tt0816692","Type":"movie","Response":"True"}"#
                .into(),
        );
    }
    (
        200,
        r#"{"Response":"False","Error":"Incorrect IMDb ID."}"#.into(),
    )
}

fn config(addr: SocketAddr, key: &str) -> ProviderConfig {
    ProviderConfig {
        api_key: Some(key.into()),
        base_url: Some(format!("http://{addr}")),
        ..Default::default()
    }
}

#[tokio::test]
async fn search_maps_results_and_years() {
    let addr = serve(mock).await;
    let items = OmdbPlugin::new()
        .search(
            SearchQuery {
                title: "Inter stellar".into(),
                year: Some(2014),
                media_type: MediaType::Movie,
                language: None,
                external_ids: Default::default(),
            },
            &config(addr, "synthetic"),
        )
        .await
        .unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].id, "tt0816692");
    assert_eq!(items[0].year, Some(2014));
    assert_eq!(items[1].year, Some(2011));
    assert_eq!(items[1].media_type, MediaType::TvShow);
    assert_eq!(items[1].poster_url, None);
}

#[tokio::test]
async fn details_and_images_are_structured() {
    let addr = serve(mock).await;
    let plugin = OmdbPlugin::new();
    let cfg = config(addr, "synthetic");
    let d = plugin
        .details(
            DetailsRequest {
                id: "tt0816692".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &cfg,
        )
        .await
        .unwrap();
    assert_eq!(d.title.as_deref(), Some("Interstellar"));
    assert_eq!(d.runtime_minutes, Some(169));
    assert_eq!(d.genres, vec!["Adventure", "Drama", "Sci-Fi"]);
    assert_eq!(d.writers.len(), 2);
    assert_eq!(d.actors[1].name, "Anne Hathaway");
    assert_eq!(d.ratings[0].votes, Some(2_100_000));
    assert_eq!(d.ratings[1].max, 100.0);
    assert_eq!(d.mpaa.as_deref(), Some("PG-13"));
    let imgs = plugin
        .images(
            ImagesRequest {
                id: "tt0816692".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &cfg,
        )
        .await
        .unwrap();
    assert_eq!(imgs.len(), 1);
}

#[tokio::test]
async fn errors_are_typed_and_do_not_leak_the_key() {
    let addr = serve(mock).await;
    let plugin = OmdbPlugin::new();
    let req = || DetailsRequest {
        id: "tt0816692".into(),
        media_type: MediaType::Movie,
        external_ids: Default::default(),
    };
    let missing = plugin.details(req(), &ProviderConfig::default()).await;
    assert_eq!(missing.unwrap_err().code, "missing_api_key");
    let bad = plugin.details(req(), &config(addr, "wrong-secret")).await;
    let err = bad.unwrap_err();
    assert_eq!(err.code, "unauthorized");
    assert!(!err.message.contains("wrong-secret"));
    let dead = ProviderConfig {
        api_key: Some("secret-key".into()),
        base_url: Some("http://127.0.0.1:1".into()),
        timeout_secs: Some(2),
        ..Default::default()
    };
    let err = plugin.details(req(), &dead).await.unwrap_err();
    assert_eq!(err.code, "network_error");
    assert!(!err.message.contains("secret-key"));
}

#[test]
fn manifest_declares_host_and_secret_field() {
    let m = OmdbPlugin::new().manifest();
    assert_eq!(m.http_hosts, vec!["www.omdbapi.com"]);
    assert!(
        m.config_schema
            .iter()
            .any(|f| f.key == "api_key" && f.secret)
    );
}
