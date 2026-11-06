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

// The fixture deliberately returns a different title/year from the filename:
// provider IDs identify the movie even when local naming is inaccurate.
async fn identification_fixture() -> (
    ProviderConfig,
    tokio::task::JoinHandle<()>,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
) {
    use http_body_util::Full;
    use hyper::{Request, Response, body::Bytes, server::conn::http1, service::service_fn};
    use hyper_util::rt::TokioIo;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = requests.clone();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let captured = captured.clone();
            tokio::spawn(async move {
                http1::Builder::new().serve_connection(TokioIo::new(stream), service_fn(move |req: Request<hyper::body::Incoming>| {
                    captured.lock().unwrap().push(req.uri().to_string());
                    async move {
                        let body = match req.uri().path() {
                            "/movie/42" => r#"{"id":42,"title":"Titre français","release_date":"1999-01-01"}"#,
                            "/find/tt0000042" => r#"{"movie_results":[{"id":42}]}"#,
                            "/find/tt0000099" => r#"{"movie_results":[]}"#,
                            "/search/movie" if req.uri().query().unwrap_or_default().contains("year=") => r#"{"results":[]}"#,
                            "/search/movie" => r#"{"results":[{"id":42,"title":"Titre français","release_date":"1999-01-01"}]}"#,
                            path => panic!("Unexpected route: {path}"),
                        };
                        Ok::<_, std::convert::Infallible>(Response::new(Full::new(Bytes::from(body))))
                    }
                })).await.unwrap();
            });
        }
    });
    (
        ProviderConfig {
            api_key: Some("fixture".into()),
            base_url: Some(format!("http://{address}")),
            ..Default::default()
        },
        server,
        requests,
    )
}

#[tokio::test]
async fn known_ids_bypass_title_search_and_report_external_lookup_failure() {
    let (config, server, requests) = identification_fixture().await;
    for (provider, id) in [("tmdb", "42"), ("imdb", "tt0000042")] {
        let results = TmdbPlugin::new()
            .search(
                SearchQuery {
                    title: "Incorrect local title".into(),
                    year: Some(2026),
                    media_type: MediaType::Movie,
                    language: None,
                    external_ids: [(provider.into(), id.into())].into(),
                },
                &config,
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "42");
        assert_eq!(results[0].title, "Titre français");
    }
    let error = TmdbPlugin::new()
        .details(
            DetailsRequest {
                id: "tt0000099".into(),
                media_type: MediaType::Movie,
                external_ids: Default::default(),
                ..Default::default()
            },
            &config,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "not_found");
    assert!(
        requests
            .lock()
            .unwrap()
            .iter()
            .all(|uri| !uri.contains("search/"))
    );
    server.abort();
}

#[tokio::test]
async fn title_search_retries_without_year_and_keeps_custom_settings() {
    let (mut config, server, requests) = identification_fixture().await;
    config.extra.insert("include_adult".into(), "true".into());
    let results = TmdbPlugin::new()
        .search(
            SearchQuery {
                title: "Titre".into(),
                year: Some(2000),
                media_type: MediaType::Movie,
                language: None,
                external_ids: Default::default(),
            },
            &config,
        )
        .await
        .unwrap();
    assert_eq!(results[0].id, "42");
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|uri| uri.contains("include_adult=true"))
    );
    assert!(requests[0].contains("year=2000"));
    assert!(!requests[1].contains("year="));
    server.abort();
}

#[test]
fn test_tmdb_unsupported_api_version() {
    let req = media_extension_sdk::ExtensionRequest {
        api_version: 2,
        operation: "manifest".into(),
        input: serde_json::Value::Null,
    };
    let res = aether_plugin_tmdb::handle_tmdb_extension_request(req);
    assert_eq!(res.api_version, 1);
    assert_eq!(res.result["error"], "unsupported_api_version");
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

/// A provider stand-in that answers only what the request asked for, like the real API:
/// `append_to_response` parts are returned when listed and never otherwise. Every request
/// is recorded as `path?query`.
async fn provider(
    answer: impl Fn(&str, &str) -> Option<String> + Send + Sync + 'static,
) -> (
    String,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    tokio::task::JoinHandle<()>,
) {
    use http_body_util::Full;
    use hyper::{
        Request, Response, StatusCode, body::Bytes, server::conn::http1, service::service_fn,
    };
    use hyper_util::rt::TokioIo;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let answer = std::sync::Arc::new(answer);
    let seen = log.clone();
    let server = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let (answer, seen) = (answer.clone(), seen.clone());
            tokio::spawn(async move {
                let _ = http1::Builder::new()
                    .serve_connection(
                        TokioIo::new(stream),
                        service_fn(move |req: Request<hyper::body::Incoming>| {
                            let (path, query) = (
                                req.uri().path().to_owned(),
                                req.uri().query().unwrap_or("").to_owned(),
                            );
                            seen.lock().unwrap().push(format!("{path}?{query}"));
                            let body = answer(&path, &query);
                            async move {
                                Ok::<_, std::convert::Infallible>(match body {
                                    Some(body) => Response::new(Full::new(Bytes::from(body))),
                                    None => Response::builder()
                                        .status(StatusCode::NOT_FOUND)
                                        .body(Full::new(Bytes::from("{}")))
                                        .unwrap(),
                                })
                            }
                        }),
                    )
                    .await;
            });
        }
    });
    (format!("http://{address}"), log, server)
}

fn config(base: &str, language: &str) -> ProviderConfig {
    ProviderConfig {
        api_key: Some("test-api-key".into()),
        base_url: Some(base.to_owned()),
        language: Some(language.into()),
        ..Default::default()
    }
}

fn movie_answer(path: &str, query: &str) -> Option<String> {
    match path {
        "/movie/438631" => {
            // Credits and external ids exist only when they were appended.
            let appended = |part: &str| query.contains("append_to_response=") && query.split("append_to_response=").nth(1).is_some_and(|v| v.split('&').next().unwrap_or("").replace("%2C", ",").split(',').any(|p| p == part));
            let mut body = serde_json::json!({
                "id": 438631, "belongs_to_collection": {"id": 726871, "name": "Dune Collection"},
                "title": "Dune", "original_title": "Dune", "original_language": "en",
                "tagline": "Beyond fear, destiny awaits.",
                "overview": "Paul Atreides, a brilliant and gifted young man...",
                "release_date": "2021-09-15", "runtime": 155, "status": "Released",
                "vote_average": 7.8, "vote_count": 11500,
                "poster_path": "/dune_poster.jpg", "backdrop_path": "/dune_backdrop.jpg",
                "genres": [{"id": 878, "name": "Science-Fiction"}, {"id": 12, "name": "Aventure"}]
            });
            if appended("credits") {
                body["credits"] = serde_json::json!({
                    "cast": [
                        {"id": 1190668, "name": "Timothée Chalamet", "character": "Paul Atreides", "profile_path": "/chalamet.jpg", "order": 0},
                        {"name": "Rebecca Ferguson", "character": "Lady Jessica", "profile_path": "/ferguson.jpg", "order": 1}
                    ],
                    "crew": [
                        {"id": 13711, "name": "Denis Villeneuve", "job": "Director", "profile_path": "/denis.jpg", "department": "Directing"},
                        {"id": 38994, "name": "Jon Spaihts", "job": "Screenplay", "profile_path": "//invalid.example/portrait.jpg", "department": "Writing"},
                        {"id": 25, "name": "Example", "job": "Producer"}
                    ]
                });
            }
            if appended("external_ids") {
                body["external_ids"] = serde_json::json!({"imdb_id": "tt1160419", "tvdb_id": 12345});
            }
            // Traps: a provider that was not asked for these must not be credited with them.
            if appended("images") {
                body["images"] = serde_json::json!({"posters": [{"file_path": "/asked.jpg", "iso_639_1": "fr"}]});
            }
            if appended("translations") {
                body["translations"] = serde_json::json!({"translations": [{"iso_639_1": "fr", "iso_3166_1": "FR", "data": {"title": "asked"}}]});
            }
            Some(body.to_string())
        }
        "/movie/438631/translations" => Some(
            serde_json::json!({"id": 438631, "translations": [
                {"iso_639_1": "fr", "iso_3166_1": "FR", "name": "Français", "english_name": "French",
                 "data": {"title": "Dune", "overview": "Paul Atreides, jeune homme brillant...", "tagline": "Au-delà de la peur."}},
                {"iso_639_1": "fr", "iso_3166_1": "CA", "name": "Français", "english_name": "French",
                 "data": {"title": "Dune (Québec)", "overview": "", "tagline": ""}},
                {"iso_639_1": "en", "iso_3166_1": "US", "name": "English", "english_name": "English",
                 "data": {"title": "Dune", "overview": "A noble family...", "tagline": ""}},
                {"iso_639_1": "de", "iso_3166_1": "DE", "name": "Deutsch", "english_name": "German", "data": {"title": "Dune DE", "overview": "x"}},
                {"iso_639_1": "ja", "iso_3166_1": "JP", "name": "日本語", "english_name": "Japanese", "data": {"title": "", "overview": "", "tagline": ""}}
            ]})
            .to_string(),
        ),
        _ => None,
    }
}

#[tokio::test]
async fn movie_details_ask_only_for_what_they_read_and_keep_regional_translations() {
    let (base, log, server) = provider(movie_answer).await;
    let plugin = TmdbPlugin::new();
    let config = config(&base, "fr-FR");
    let request = |languages: &[&str]| DetailsRequest {
        id: "438631".into(),
        media_type: MediaType::Movie,
        languages: languages.iter().map(|l| (*l).into()).collect(),
        ..Default::default()
    };

    // The Core asks for nothing extra: one request, no translation call, no images.
    let details = plugin.details(request(&[]), &config).await.unwrap();
    assert_eq!(log.lock().unwrap().len(), 1);
    {
        let sent = log.lock().unwrap();
        assert!(
            sent[0].contains("append_to_response=credits%2Cexternal_ids"),
            "{}",
            sent[0]
        );
        assert!(!sent[0].contains("images") && !sent[0].contains("translations"));
        assert!(sent[0].contains("language=fr-FR"));
    }
    assert_eq!(details.provider, "tmdb");
    assert_eq!(details.provider_id, "438631");
    assert_eq!(details.collection.as_ref().unwrap().id, "tmdb:726871");
    assert_eq!(details.title.as_deref(), Some("Dune"));
    assert_eq!(details.locale.as_deref(), Some("fr-FR"));
    assert_eq!(details.original_language.as_deref(), Some("en"));
    assert_eq!(details.year, Some(2021));
    assert_eq!(details.runtime_minutes, Some(155));
    assert_eq!(details.directors, vec!["Denis Villeneuve"]);
    assert_eq!(details.writers, vec!["Jon Spaihts"]);
    assert_eq!(details.actors.len(), 2);
    assert_eq!(details.actors[0].role.as_deref(), Some("Paul Atreides"));
    assert_eq!(
        details.external_ids.get("imdb"),
        Some(&"tt1160419".to_string())
    );
    assert_eq!(details.ratings[0].value, 7.8);
    // Only the provider's own pick, with an unknown language: never the request's language.
    assert_eq!(
        (
            details.posters.len(),
            details.backdrops.len(),
            details.images.len()
        ),
        (1, 1, 0)
    );
    assert_eq!(details.posters[0].language, None);
    assert!(details.translations.is_empty());

    // Two extra languages: exactly one more request returns all of them, with regions.
    log.lock().unwrap().clear();
    let details = plugin
        .details(
            request(&["fr-FR", "fr-CA", "en-GB", "ja", "bad tag"]),
            &config,
        )
        .await
        .unwrap();
    assert_eq!(log.lock().unwrap().len(), 2);
    assert!(log.lock().unwrap()[1].starts_with("/movie/438631/translations?"));
    let keys: Vec<_> = details.translations.keys().map(String::as_str).collect();
    // fr-FR and fr-CA stay distinct; en-GB is served by the provider's en-US; German was not
    // asked for; Japanese has only empty strings, which are not translations.
    assert_eq!(keys, ["en-US", "fr-CA", "fr-FR"]);
    assert_eq!(
        details.translations["fr-CA"].title.as_deref(),
        Some("Dune (Québec)")
    );
    assert_eq!(details.translations["fr-CA"].overview, None);
    assert_eq!(
        details.translations["fr-FR"].tagline.as_deref(),
        Some("Au-delà de la peur.")
    );

    // Wire form read by the Core.
    let wire = plugin.details_value(request(&[]), &config).await.unwrap();
    assert_eq!(wire["originalLanguage"], "en");
    assert_eq!(wire["locale"], "fr-FR");
    assert_eq!(wire["actors"][0]["providerId"], "tmdb:1190668");
    assert!(wire["actors"][1].get("providerId").is_none());
    assert_eq!(wire["crewCredits"].as_array().unwrap().len(), 2);
    assert_eq!(
        wire["crewCredits"][0]["thumbUrl"],
        "https://image.tmdb.org/t/p/w185/denis.jpg"
    );
    assert!(wire["crewCredits"][1]["thumbUrl"].is_null());
    server.abort();
}

#[tokio::test]
async fn a_failing_translation_call_keeps_the_main_language_but_auth_errors_are_reported() {
    let (base, _log, server) = provider(|path, query| {
        if path == "/movie/438631/translations" {
            None
        } else {
            movie_answer(path, query)
        }
    })
    .await;
    let request = DetailsRequest {
        id: "438631".into(),
        languages: vec!["en-US".into()],
        ..Default::default()
    };
    let details = TmdbPlugin::new()
        .details(request, &config(&base, "fr-FR"))
        .await
        .unwrap();
    assert_eq!(details.title.as_deref(), Some("Dune"));
    assert!(details.translations.is_empty());
    server.abort();
}

fn tv_answer(seasons: u32) -> impl Fn(&str, &str) -> Option<String> + Send + Sync + 'static {
    move |path, query| {
        let language = query
            .split('&')
            .find_map(|p| p.strip_prefix("language="))
            .unwrap_or("");
        match path {
            "/find/tt7777777" => Some(r#"{"tv_results":[{"id":777}]}"#.into()),
            "/tv/777" => Some(
                serde_json::json!({
                    "id": 777, "name": "Série", "original_name": "Show", "original_language": "en",
                    "first_air_date": "2020-01-01", "external_ids": {"imdb_id": "tt7777777"},
                    "seasons": std::iter::once(0).chain(1..=seasons).map(|n| serde_json::json!({
                        "season_number": n, "name": format!("Saison {n}"), "overview": "", "poster_path": null
                    })).collect::<Vec<_>>()
                })
                .to_string(),
            ),
            "/tv/777/translations" => Some(
                r#"{"translations":[{"iso_639_1":"en","iso_3166_1":"US","data":{"name":"Show","overview":"A show."}}]}"#.into(),
            ),
            p if p.starts_with("/tv/777/season/") => {
                let number: u32 = p.rsplit('/').next().unwrap().parse().unwrap();
                let english = language.starts_with("en");
                Some(
                    serde_json::json!({
                        "name": if english { format!("Season {number}") } else { format!("Saison {number}") },
                        "overview": "",
                        "episodes": (1..=2).map(|e| serde_json::json!({
                            "episode_number": e,
                            "name": if english { format!("Episode {e}") } else { format!("Épisode {e}") },
                            "overview": if english { "English plot" } else { "Résumé" },
                            "air_date": "2020-01-01", "runtime": 42, "vote_average": 7.0
                        })).collect::<Vec<_>>()
                    })
                    .to_string(),
                )
            }
            _ => None,
        }
    }
}

#[tokio::test]
async fn series_seasons_use_the_resolved_id_and_other_languages_land_on_episodes() {
    let (base, log, server) = provider(tv_answer(2)).await;
    // Search flow: the identifier is known only through IMDb, so `request.id` is empty.
    let request = DetailsRequest {
        id: String::new(),
        media_type: MediaType::TvShow,
        external_ids: [("imdb".into(), "tt7777777".into())].into(),
        languages: vec!["fr-FR".into(), "en-US".into()],
    };
    let details = TmdbPlugin::new()
        .details(request, &config(&base, "fr-FR"))
        .await
        .unwrap();
    let sent = log.lock().unwrap().clone();
    assert!(
        sent.iter().all(|u| !u.contains("tv//")),
        "season calls must use the resolved id: {sent:?}"
    );
    // find, series, translations, seasons 1, 2 and the specials last, then English for 1 and 2.
    let paths: Vec<&str> = sent.iter().map(|u| u.split('?').next().unwrap()).collect();
    assert_eq!(
        paths,
        [
            "/find/tt7777777",
            "/tv/777",
            "/tv/777/translations",
            "/tv/777/season/1",
            "/tv/777/season/2",
            "/tv/777/season/0",
            "/tv/777/season/1",
            "/tv/777/season/2",
            "/tv/777/season/0"
        ]
    );
    assert_eq!(
        details
            .seasons
            .iter()
            .map(|s| s.season_number)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let season = &details.seasons[1];
    assert_eq!(season.episodes[0].title.as_deref(), Some("Épisode 1"));
    assert_eq!(
        season.translations["en-US"].title.as_deref(),
        Some("Season 1")
    );
    assert_eq!(
        season.episodes[1].translations["en-US"].title.as_deref(),
        Some("Episode 2")
    );
    assert_eq!(
        season.episodes[1].translations["en-US"].overview.as_deref(),
        Some("English plot")
    );
    assert_eq!(details.translations["en-US"].title.as_deref(), Some("Show"));
    server.abort();
}

#[tokio::test]
async fn long_series_never_exceed_the_hosts_request_quota() {
    let (base, log, server) = provider(tv_answer(30)).await;
    let request = DetailsRequest {
        id: "777".into(),
        media_type: MediaType::TvShow,
        languages: vec!["fr-FR".into(), "en-US".into()],
        ..Default::default()
    };
    let details = TmdbPlugin::new()
        .details(request, &config(&base, "fr-FR"))
        .await
        .unwrap();
    // Core allows ten HTTP requests per invocation; staying inside it is the plugin's job.
    assert_eq!(log.lock().unwrap().len(), 10);
    assert_eq!(details.seasons.len(), 31, "every season is listed ...");
    let with_episodes: Vec<u32> = details
        .seasons
        .iter()
        .filter(|s| !s.episodes.is_empty())
        .map(|s| s.season_number)
        .collect();
    // ... the first ones carry episodes (ascending, specials not before real seasons).
    assert_eq!(with_episodes, [1, 2, 3, 4, 5, 6, 7, 8]);
    server.abort();
}

#[tokio::test]
async fn output_stays_under_the_abi_cap_by_dropping_the_least_valuable_data_first() {
    let big = "x".repeat(900);
    let (base, _log, server) = provider(move |path, query| {
        if let Some(rest) = path.strip_prefix("/tv/777/season/") {
            let _ = rest;
            let language = query
                .split('&')
                .find_map(|p| p.strip_prefix("language="))
                .unwrap_or("");
            return Some(
                serde_json::json!({"name": "S", "episodes": (1..=60).map(|e| serde_json::json!({
                    "episode_number": e, "name": format!("{language} {e}"), "overview": big.clone()
                })).collect::<Vec<_>>()})
                .to_string(),
            );
        }
        tv_answer(3)(path, query)
    })
    .await;
    let request = DetailsRequest {
        id: "777".into(),
        media_type: MediaType::TvShow,
        languages: vec!["fr-FR".into(), "en-US".into()],
        ..Default::default()
    };
    let wire = TmdbPlugin::new()
        .details_value(request, &config(&base, "fr-FR"))
        .await
        .unwrap();
    let size = serde_json::to_vec(&wire).unwrap().len();
    assert!(size <= 64 * 1024 - 1024, "{size} bytes");
    assert_eq!(
        wire["title"], "Série",
        "identity and titles are never dropped"
    );
    assert_eq!(wire["externalIds"]["imdb"], "tt7777777");
    assert!(
        wire["seasons"].as_array().unwrap().len() >= 3,
        "seasons stay listed"
    );
    server.abort();
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
                    // The policy is forwarded so the provider returns only those languages.
                    assert_eq!(req.uri().query(), Some("include_image_language=fr%2Cen%2Cnull&api_key=fixture"));
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
