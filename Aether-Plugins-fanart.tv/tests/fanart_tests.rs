use aether_plugin_api::{
    DetailsRequest, ImageType, MediaType, MetadataProviderPlugin, ProviderConfig,
};
use aether_plugin_fanart::FanartTvPlugin;

#[test]
fn test_fanart_manifest() {
    let plugin = FanartTvPlugin::new();
    let manifest = plugin.manifest();
    assert_eq!(manifest.id, "fanart-tv");
    assert_eq!(manifest.provider, "fanart.tv");
    assert_eq!(manifest.default_priority, 60);
    assert!(
        manifest
            .capabilities
            .contains(&aether_plugin_api::PluginCapability::Details)
    );
    assert!(
        manifest
            .capabilities
            .contains(&aether_plugin_api::PluginCapability::Images)
    );
}

#[tokio::test]
async fn test_fanart_missing_api_key() {
    let plugin = FanartTvPlugin::new();
    let config = ProviderConfig {
        api_key: None,
        ..Default::default()
    };
    unsafe { std::env::remove_var("FANART_TV_API_KEY") };

    let res = plugin
        .details(
            DetailsRequest {
                id: "27205".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config,
        )
        .await;

    assert!(res.is_err());
    let err = res.unwrap_err();
    assert_eq!(err.code, "missing_api_key");
}

#[tokio::test]
async fn test_fanart_movie_details_mock() {
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Request, Response, StatusCode};
    use hyper_util::rt::TokioIo;
    use std::convert::Infallible;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => break,
            };
            let io = TokioIo::new(stream);
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .serve_connection(
                        io,
                        service_fn(|req: Request<hyper::body::Incoming>| async move {
                            let path = req.uri().path();
                            if path.contains("/movies/27205") {
                                let mock_json = r#"{
                                    "name": "Inception",
                                    "tmdb_id": "27205",
                                    "imdb_id": "tt1375666",
                                    "movieposter": [
                                        {"id": "1", "url": "https://assets.fanart.tv/fanart/movies/27205/movieposter/inception-1.jpg", "lang": "fr", "likes": "15"},
                                        {"id": "2", "url": "https://assets.fanart.tv/fanart/movies/27205/movieposter/inception-2.jpg", "lang": "en", "likes": "10"}
                                    ],
                                    "moviebackground": [
                                        {"id": "3", "url": "https://assets.fanart.tv/fanart/movies/27205/moviebackground/inception-bg.jpg", "lang": "00", "likes": "42"}
                                    ],
                                    "hdmovielogo": [
                                        {"id": "4", "url": "https://assets.fanart.tv/fanart/movies/27205/hdmovielogo/inception-logo.png", "lang": "fr", "likes": "28"}
                                    ],
                                    "hdmovieart": [
                                        {"id": "5", "url": "https://assets.fanart.tv/fanart/movies/27205/hdmovieart/inception-art.png", "lang": "en", "likes": "8"}
                                    ],
                                    "moviebanner": [
                                        {"id": "6", "url": "https://assets.fanart.tv/fanart/movies/27205/moviebanner/inception-banner.jpg", "lang": "en", "likes": "5"}
                                    ],
                                    "moviethumb": [
                                        {"id": "7", "url": "https://assets.fanart.tv/fanart/movies/27205/moviethumb/inception-thumb.jpg", "lang": "en", "likes": "6"}
                                    ],
                                    "moviedisc": [
                                        {"id": "8", "url": "https://assets.fanart.tv/fanart/movies/27205/moviedisc/inception-disc.png", "lang": "fr", "likes": "12"}
                                    ]
                                }"#;
                                Ok::<_, Infallible>(Response::builder()
                                    .header("Content-Type", "application/json")
                                    .body(Full::new(Bytes::from(mock_json)))
                                    .unwrap())
                            } else {
                                Ok::<_, Infallible>(Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .body(Full::new(Bytes::from("{}")))
                                    .unwrap())
                            }
                        }),
                    )
                    .await;
            });
        }
    });

    let plugin = FanartTvPlugin::new();
    let config = ProviderConfig {
        api_key: Some("mock-fanart-key".into()),
        base_url: Some(format!("http://{addr}")),
        ..Default::default()
    };

    let details = plugin
        .details(
            DetailsRequest {
                id: "27205".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config,
        )
        .await
        .unwrap();

    assert_eq!(details.provider, "fanart.tv");
    assert_eq!(details.provider_id, "27205");
    assert_eq!(details.title.as_deref(), Some("Inception"));
    assert_eq!(details.external_ids.get("tmdb"), Some(&"27205".to_string()));
    assert_eq!(
        details.external_ids.get("imdb"),
        Some(&"tt1375666".to_string())
    );

    // Verify categorisation
    assert_eq!(details.posters.len(), 2);
    assert_eq!(details.posters[0].language.as_deref(), Some("fr"));
    assert_eq!(details.posters[0].rating, Some(15.0));

    assert_eq!(details.backdrops.len(), 1);
    assert_eq!(details.backdrops[0].language, None); // "00" converted to None
    assert_eq!(details.backdrops[0].rating, Some(42.0));

    // Verify all artwork was aggregated
    assert_eq!(details.images.len(), 8);

    let has_logo = details
        .images
        .iter()
        .any(|img| img.image_type == ImageType::Logo);
    let has_clearart = details
        .images
        .iter()
        .any(|img| img.image_type == ImageType::ClearArt);
    let has_banner = details
        .images
        .iter()
        .any(|img| img.image_type == ImageType::Banner);
    let has_thumb = details
        .images
        .iter()
        .any(|img| img.image_type == ImageType::Thumb);
    let has_disc = details
        .images
        .iter()
        .any(|img| img.image_type == ImageType::DiscArt);

    assert!(has_logo);
    assert!(has_clearart);
    assert!(has_banner);
    assert!(has_thumb);
    assert!(has_disc);
}
