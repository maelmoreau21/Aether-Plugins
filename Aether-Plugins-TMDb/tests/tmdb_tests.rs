use aether_plugin_api::{
    DetailsRequest, MediaType, MetadataProviderPlugin, ProviderConfig, SearchQuery,
};
use aether_plugin_tmdb::TmdbPlugin;

#[test]
fn test_tmdb_manifest() {
    let plugin = TmdbPlugin::new();
    let manifest = plugin.manifest();
    assert_eq!(manifest.id, "tmdb");
    assert_eq!(manifest.provider, "tmdb");
    assert_eq!(manifest.default_priority, 100);
    assert!(manifest.capabilities.len() >= 3);
}

#[tokio::test]
async fn test_tmdb_missing_api_key() {
    let plugin = TmdbPlugin::new();
    let config = ProviderConfig {
        api_key: None,
        ..Default::default()
    };
    // Ensure TMDB_API_KEY env is not set or empty during this test
    unsafe { std::env::remove_var("TMDB_API_KEY") };

    let search_res = plugin
        .search(
            SearchQuery {
                title: "Dune".into(),
                year: Some(2021),
                media_type: MediaType::Movie,
                language: None,
                external_ids: Default::default(),
            },
            &config,
        )
        .await;

    assert!(search_res.is_err());
    let err = search_res.unwrap_err();
    assert_eq!(err.code, "missing_api_key");
}

#[tokio::test]
async fn test_tmdb_details_mock_server() {
    // Spin up an in-process local mock server to test TMDb deserialization and parsing end-to-end
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
                            if path.contains("/movie/438631") {
                                let mock_json = r#"{
                                    "id": 438631,
                                    "title": "Dune",
                                    "original_title": "Dune",
                                    "tagline": "Beyond fear, destiny awaits.",
                                    "overview": "Paul Atreides, a brilliant and gifted young man...",
                                    "release_date": "2021-09-15",
                                    "runtime": 155,
                                    "status": "Released",
                                    "vote_average": 7.8,
                                    "vote_count": 11500,
                                    "genres": [{"id": 878, "name": "Science-Fiction"}, {"id": 12, "name": "Aventure"}],
                                    "credits": {
                                        "cast": [
                                            {"name": "Timothée Chalamet", "character": "Paul Atreides", "profile_path": "/chalamet.jpg", "order": 0},
                                            {"name": "Rebecca Ferguson", "character": "Lady Jessica", "profile_path": "/ferguson.jpg", "order": 1}
                                        ],
                                        "crew": [
                                            {"name": "Denis Villeneuve", "job": "Director", "department": "Directing"},
                                            {"name": "Jon Spaihts", "job": "Screenplay", "department": "Writing"}
                                        ]
                                    },
                                    "images": {
                                        "posters": [
                                            {"file_path": "/dune_poster_fr.jpg", "width": 2000, "height": 3000, "iso_639_1": "fr", "vote_average": 8.5, "vote_count": 120}
                                        ],
                                        "backdrops": [
                                            {"file_path": "/dune_backdrop.jpg", "width": 3840, "height": 2160, "iso_639_1": null, "vote_average": 8.9, "vote_count": 250}
                                        ],
                                        "logos": [
                                            {"file_path": "/dune_logo.png", "width": 1920, "height": 800, "iso_639_1": "en", "vote_average": 7.5, "vote_count": 40}
                                        ]
                                    },
                                    "external_ids": {
                                        "imdb_id": "tt1160419",
                                        "tvdb_id": 12345
                                    }
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

    let plugin = TmdbPlugin::new();
    let config = ProviderConfig {
        api_key: Some("test-api-key".into()),
        base_url: Some(format!("http://{addr}")),
        language: Some("fr-FR".into()),
        ..Default::default()
    };

    let details = plugin
        .details(
            DetailsRequest {
                id: "438631".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config,
        )
        .await
        .unwrap();

    assert_eq!(details.provider, "tmdb");
    assert_eq!(details.provider_id, "438631");
    assert_eq!(details.title.as_deref(), Some("Dune"));
    assert_eq!(details.year, Some(2021));
    assert_eq!(details.runtime_minutes, Some(155));
    assert_eq!(details.directors, vec!["Denis Villeneuve"]);
    assert_eq!(details.writers, vec!["Jon Spaihts"]);
    assert_eq!(details.actors.len(), 2);
    assert_eq!(details.actors[0].name, "Timothée Chalamet");
    assert_eq!(details.actors[0].role.as_deref(), Some("Paul Atreides"));
    assert_eq!(
        details.external_ids.get("imdb"),
        Some(&"tt1160419".to_string())
    );
    assert_eq!(details.ratings[0].value, 7.8);
    assert_eq!(details.posters.len(), 1);
    assert_eq!(details.backdrops.len(), 1);
    assert_eq!(details.images.len(), 3);
}

#[tokio::test]
async fn image_collection_keeps_localized_variants_and_neutral_artwork() {
    use aether_plugin_api::{ImageType, ImagesRequest};
    use http_body_util::Full;
    use hyper::{Request, Response, body::Bytes, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    use std::convert::Infallible;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                http1::Builder::new().serve_connection(TokioIo::new(stream), service_fn(|req: Request<hyper::body::Incoming>| async move {
                    assert_eq!(req.uri().path(), "/movie/42/images");
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(r#"{"posters":[{"file_path":"/fr1.jpg","iso_639_1":"fr"},{"file_path":"/fr2.jpg","iso_639_1":"fr"},{"file_path":"/en.jpg","iso_639_1":"en"},{"file_path":"/de.jpg","iso_639_1":"de"}],"backdrops":[{"file_path":"/neutral.jpg","iso_639_1":null}],"logos":[{"file_path":"/logo.png","iso_639_1":"fr"}]}"#))))
                })).await.unwrap();
            });
        }
    });
    let plugin = TmdbPlugin::new();
    let config = ProviderConfig {
        api_key: Some("fixture".into()),
        base_url: Some(format!("http://{address}")),
        language: Some("fr-FR".into()),
        fallback_languages: vec!["en".into()],
        ..Default::default()
    };
    let images = plugin
        .images(
            ImagesRequest {
                id: String::new(),
                media_type: MediaType::Movie,
                external_ids: [("tmdb".into(), "42".into())].into(),
            },
            &config,
        )
        .await
        .unwrap();
    assert_eq!(
        images
            .iter()
            .filter(|v| v.image_type == ImageType::Poster)
            .count(),
        3
    );
    assert!(images.iter().any(|v| v.image_type == ImageType::Logo));
    assert!(
        images
            .iter()
            .any(|v| v.image_type == ImageType::Backdrop && v.language.is_none())
    );
    assert!(!images.iter().any(|v| v.language.as_deref() == Some("de")));
    server.abort();
}
