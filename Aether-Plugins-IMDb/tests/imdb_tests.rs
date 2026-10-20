use aether_plugin_api::{
    DetailsRequest, MediaType, MetadataProviderPlugin, ProviderConfig, SearchQuery,
};
use aether_plugin_imdb::ImdbPlugin;

#[test]
fn test_imdb_manifest() {
    let plugin = ImdbPlugin::new();
    let manifest = plugin.manifest();
    assert_eq!(manifest.id, "imdb");
    assert_eq!(manifest.provider, "imdb");
    assert_eq!(manifest.default_priority, 80);
    assert!(manifest.capabilities.len() >= 3);
}

#[tokio::test]
async fn test_imdb_suggestion_mock() {
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
                            if path.contains("interstellar") {
                                let mock_json = r#"{
                                    "d": [
                                        {
                                            "id": "tt0816692",
                                            "l": "Interstellar",
                                            "s": "Matthew McConaughey, Anne Hathaway",
                                            "y": 2014,
                                            "q": "feature",
                                            "i": {
                                                "imageUrl": "https://m.media-amazon.com/images/M/interstellar.jpg",
                                                "height": 1500,
                                                "width": 1000
                                            }
                                        }
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

    let plugin = ImdbPlugin::new();
    let config = ProviderConfig {
        base_url: Some(format!("http://{addr}")),
        ..Default::default()
    };

    let items = plugin
        .search(
            SearchQuery {
                title: "Interstellar".into(),
                year: Some(2014),
                media_type: MediaType::Movie,
                language: None,
                external_ids: Default::default(),
            },
            &config,
        )
        .await
        .unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "tt0816692");
    assert_eq!(items[0].title, "Interstellar");
    assert_eq!(items[0].year, Some(2014));
    assert_eq!(items[0].media_type, MediaType::Movie);
    assert!(
        items[0]
            .overview
            .as_ref()
            .unwrap()
            .contains("Matthew McConaughey")
    );
    assert_eq!(
        items[0].poster_url.as_deref(),
        Some("https://m.media-amazon.com/images/M/interstellar.jpg")
    );
}

#[tokio::test]
async fn test_imdb_details_mock() {
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
                            if path.contains("tt0816692") {
                                let mock_html = r#"
                                <!DOCTYPE html>
                                <html>
                                <head>
                                <script type="application/ld+json">
                                {
                                  "@context": "https://schema.org",
                                  "@type": "Movie",
                                  "name": "Interstellar",
                                  "alternateName": "Interstellaire",
                                  "image": "https://m.media-amazon.com/images/M/poster.jpg",
                                  "description": "When Earth becomes uninhabitable in the future, a farmer and ex-NASA pilot, Joseph Cooper, is tasked to pilot a spacecraft...",
                                  "datePublished": "2014-11-05",
                                  "duration": "PT2H49M",
                                  "contentRating": "PG-13",
                                  "genre": ["Adventure", "Drama", "Sci-Fi"],
                                  "actor": [
                                    {"@type": "Person", "name": "Matthew McConaughey"},
                                    {"@type": "Person", "name": "Anne Hathaway"}
                                  ],
                                  "director": [
                                    {"@type": "Person", "name": "Christopher Nolan"}
                                  ],
                                  "creator": [
                                    {"@type": "Person", "name": "Jonathan Nolan"},
                                    {"@type": "Person", "name": "Christopher Nolan"}
                                  ],
                                  "aggregateRating": {
                                    "@type": "AggregateRating",
                                    "ratingValue": 8.7,
                                    "bestRating": 10.0,
                                    "ratingCount": 2100000
                                  }
                                }
                                </script>
                                </head>
                                <body></body>
                                </html>
                                "#;
                                Ok::<_, Infallible>(Response::builder()
                                    .header("Content-Type", "text/html; charset=utf-8")
                                    .body(Full::new(Bytes::from(mock_html)))
                                    .unwrap())
                            } else {
                                Ok::<_, Infallible>(Response::builder()
                                    .status(StatusCode::NOT_FOUND)
                                    .body(Full::new(Bytes::from("Not Found")))
                                    .unwrap())
                            }
                        }),
                    )
                    .await;
            });
        }
    });

    let plugin = ImdbPlugin::new();
    let config = ProviderConfig {
        base_url: Some(format!("http://{addr}")),
        ..Default::default()
    };

    let details = plugin
        .details(
            DetailsRequest {
                id: "tt0816692".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
            },
            &config,
        )
        .await
        .unwrap();

    assert_eq!(details.provider, "imdb");
    assert_eq!(details.provider_id, "tt0816692");
    assert_eq!(details.title.as_deref(), Some("Interstellar"));
    assert_eq!(details.original_title.as_deref(), Some("Interstellaire"));
    assert_eq!(details.year, Some(2014));
    assert_eq!(details.runtime_minutes, Some(169)); // 2 hours 49 mins = 169 mins
    assert_eq!(details.directors, vec!["Christopher Nolan"]);
    assert_eq!(details.writers, vec!["Jonathan Nolan", "Christopher Nolan"]);
    assert_eq!(details.actors.len(), 2);
    assert_eq!(details.actors[0].name, "Matthew McConaughey");
    assert_eq!(details.actors[1].name, "Anne Hathaway");
    assert_eq!(details.ratings[0].value, 8.7);
    assert_eq!(details.ratings[0].votes, Some(2100000));
    assert_eq!(details.mpaa.as_deref(), Some("PG-13"));
    assert_eq!(details.genres, vec!["Adventure", "Drama", "Sci-Fi"]);
    assert_eq!(details.posters.len(), 1);
    assert_eq!(
        details.posters[0].url,
        "https://m.media-amazon.com/images/M/poster.jpg"
    );
}
