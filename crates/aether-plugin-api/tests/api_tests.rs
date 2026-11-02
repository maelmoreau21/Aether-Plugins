use aether_plugin_api::{
    API_VERSION, ActorCredit, DetailsRequest, ImageType, ImagesRequest, MediaImage, MediaType,
    MetadataProviderPlugin, PluginCapability, PluginError, PluginManifest, PluginRequest,
    PluginResponse, ProviderConfig, ProviderRating, SearchQuery, SearchResultItem,
    StructuredMetadata, handle_request,
};
use async_trait::async_trait;

struct MockPlugin;

#[async_trait]
impl MetadataProviderPlugin for MockPlugin {
    fn manifest(&self) -> PluginManifest {
        PluginManifest {
            api_version: API_VERSION,
            id: "mock".into(),
            name: "Mock Provider".into(),
            version: "0.1.0".into(),
            provider: "mock".into(),
            description: "Mock test provider".into(),
            author: "Tester".into(),
            license: "MIT".into(),
            capabilities: vec![
                PluginCapability::Search,
                PluginCapability::Details,
                PluginCapability::Images,
            ],
            supported_media_types: vec![MediaType::Movie, MediaType::TvShow],
            default_priority: 50,
            config_schema: vec![],
            ..Default::default()
        }
    }

    async fn search(
        &self,
        query: SearchQuery,
        _config: &ProviderConfig,
    ) -> Result<Vec<SearchResultItem>, PluginError> {
        Ok(vec![SearchResultItem {
            id: "mock-123".into(),
            title: query.title,
            original_title: None,
            year: query.year,
            media_type: query.media_type,
            overview: Some("Mock overview".into()),
            poster_url: Some("https://example.com/poster.jpg".into()),
            score: Some(8.5),
            artist: None,
        }])
    }

    async fn details(
        &self,
        request: DetailsRequest,
        _config: &ProviderConfig,
    ) -> Result<StructuredMetadata, PluginError> {
        if request.id == "error-404" {
            return Err(PluginError::not_found(&request.id));
        }

        Ok(StructuredMetadata {
            provider: "mock".into(),
            provider_id: request.id,
            external_ids: Default::default(),
            title: Some("Inception".into()),
            original_title: Some("Inception".into()),
            tagline: Some("Your mind is the scene of the crime.".into()),
            synopsis: Some(
                "A thief who steals corporate secrets through dream-sharing technology...".into(),
            ),
            year: Some(2010),
            release_date: Some("2010-07-16".into()),
            runtime_minutes: Some(148),
            genres: vec!["Action".into(), "Science-Fiction".into()],
            directors: vec!["Christopher Nolan".into()],
            writers: vec!["Christopher Nolan".into()],
            actors: vec![ActorCredit {
                name: "Leonardo DiCaprio".into(),
                role: Some("Cobb".into()),
                thumb_url: Some("https://example.com/leo.jpg".into()),
                order: Some(0),
            }],
            ratings: vec![ProviderRating {
                provider: "mock".into(),
                value: 8.8,
                max: 10.0,
                votes: Some(2500000),
            }],
            mpaa: Some("PG-13".into()),
            status: Some("Released".into()),
            posters: vec![MediaImage {
                url: "https://example.com/poster.jpg".into(),
                image_type: ImageType::Poster,
                width: Some(1000),
                height: Some(1500),
                language: Some("fr".into()),
                rating: Some(9.2),
                vote_count: Some(500),
                is_primary: true,
            }],
            backdrops: vec![MediaImage {
                url: "https://example.com/backdrop.jpg".into(),
                image_type: ImageType::Backdrop,
                width: Some(3840),
                height: Some(2160),
                language: None,
                rating: Some(9.0),
                vote_count: Some(300),
                is_primary: true,
            }],
            images: vec![],
            seasons: vec![],
            translations: Default::default(),
        })
    }

    async fn images(
        &self,
        request: ImagesRequest,
        config: &ProviderConfig,
    ) -> Result<Vec<MediaImage>, PluginError> {
        let det = self
            .details(
                DetailsRequest {
                    id: request.id,
                    media_type: request.media_type,
                    external_ids: request.external_ids,
                },
                config,
            )
            .await?;
        let mut all = det.posters;
        all.extend(det.backdrops);
        Ok(all)
    }
}

#[tokio::test]
async fn test_manifest_dispatch() {
    let plugin = MockPlugin;
    let req = PluginRequest {
        api_version: API_VERSION,
        operation: "manifest".into(),
        input: serde_json::Value::Null,
        config: ProviderConfig::default(),
    };
    let json_req = serde_json::to_string(&req).unwrap();
    let res_str = handle_request(&plugin, &json_req).await;
    let res: PluginResponse<PluginManifest> = serde_json::from_str(&res_str).unwrap();

    assert!(res.success);
    let manifest = res.data.unwrap();
    assert_eq!(manifest.id, "mock");
    assert_eq!(manifest.provider, "mock");
    assert_eq!(manifest.default_priority, 50);
}

#[tokio::test]
async fn test_search_dispatch() {
    let plugin = MockPlugin;
    let search_q = SearchQuery {
        title: "Inception".into(),
        year: Some(2010),
        media_type: MediaType::Movie,
        language: Some("fr-FR".into()),
        external_ids: Default::default(),
    };
    let req = PluginRequest {
        api_version: API_VERSION,
        operation: "search".into(),
        input: serde_json::to_value(search_q).unwrap(),
        config: ProviderConfig::default(),
    };
    let json_req = serde_json::to_string(&req).unwrap();
    let res_str = handle_request(&plugin, &json_req).await;
    let res: PluginResponse<Vec<SearchResultItem>> = serde_json::from_str(&res_str).unwrap();

    assert!(res.success);
    let items = res.data.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "mock-123");
    assert_eq!(items[0].title, "Inception");
    assert_eq!(items[0].year, Some(2010));
}

#[tokio::test]
async fn test_details_dispatch() {
    let plugin = MockPlugin;
    let details_q = DetailsRequest {
        id: "mock-123".into(),
        media_type: MediaType::Movie,
        external_ids: Default::default(),
    };
    let req = PluginRequest {
        api_version: API_VERSION,
        operation: "details".into(),
        input: serde_json::to_value(details_q).unwrap(),
        config: ProviderConfig::default(),
    };
    let json_req = serde_json::to_string(&req).unwrap();
    let res_str = handle_request(&plugin, &json_req).await;
    let res: PluginResponse<StructuredMetadata> = serde_json::from_str(&res_str).unwrap();

    assert!(res.success);
    let meta = res.data.unwrap();
    assert_eq!(meta.title.as_deref(), Some("Inception"));
    assert_eq!(meta.year, Some(2010));
    assert_eq!(meta.runtime_minutes, Some(148));
    assert_eq!(meta.directors, vec!["Christopher Nolan"]);
    assert_eq!(meta.actors.len(), 1);
    assert_eq!(meta.actors[0].name, "Leonardo DiCaprio");
    assert_eq!(meta.actors[0].role.as_deref(), Some("Cobb"));
    assert_eq!(meta.ratings[0].value, 8.8);
    assert_eq!(meta.posters.len(), 1);
    assert_eq!(meta.backdrops.len(), 1);
}

#[tokio::test]
async fn test_error_dispatch() {
    let plugin = MockPlugin;
    let details_q = DetailsRequest {
        id: "error-404".into(),
        media_type: MediaType::Movie,
        external_ids: Default::default(),
    };
    let req = PluginRequest {
        api_version: API_VERSION,
        operation: "details".into(),
        input: serde_json::to_value(details_q).unwrap(),
        config: ProviderConfig::default(),
    };
    let json_req = serde_json::to_string(&req).unwrap();
    let res_str = handle_request(&plugin, &json_req).await;
    let res: PluginResponse<StructuredMetadata> = serde_json::from_str(&res_str).unwrap();

    assert!(!res.success);
    assert!(res.data.is_none());
    let err = res.error.unwrap();
    assert_eq!(err.code, "not_found");
}

#[tokio::test]
async fn test_unsupported_api_version() {
    let plugin = MockPlugin;
    let req = PluginRequest {
        api_version: 999,
        operation: "manifest".into(),
        input: serde_json::Value::Null,
        config: ProviderConfig::default(),
    };
    let json_req = serde_json::to_string(&req).unwrap();
    let res_str = handle_request(&plugin, &json_req).await;
    let res: PluginResponse<()> = serde_json::from_str(&res_str).unwrap();

    assert!(!res.success);
    assert_eq!(res.error.unwrap().code, "unsupported_api_version");
}

#[test]
fn image_policy_retains_variants_and_regional_language_fallbacks() {
    let config = ProviderConfig::from_extension_input(&serde_json::json!({
        "language":"fr-CA", "languages":["fr", "en"], "includeNeutral":true,
        "config":{"api_key":"fixture", "fallbackLanguages":["ja"]}
    }));
    assert_eq!(config.api_key.as_deref(), Some("fixture"));
    assert_eq!(config.image_languages(), vec!["fr-ca", "fr", "en", "ja"]);
    let images: Vec<MediaImage> = serde_json::from_value(serde_json::json!([
        {"url":"neutral", "imageType":"backdrop"},
        {"url":"de", "imageType":"poster", "language":"de"},
        {"url":"en", "imageType":"logo", "language":"en"},
        {"url":"fr1", "imageType":"poster", "language":"fr"},
        {"url":"fr2", "imageType":"poster", "language":"fr"},
        {"url":"regional", "imageType":"thumb", "language":"fr-CA"},
        {"url":"ja", "imageType":"clear_art", "language":"ja"}
    ]))
    .unwrap();
    let selected = config.select_images(images.clone());
    assert_eq!(
        selected.iter().map(|v| v.url.as_str()).collect::<Vec<_>>(),
        vec!["regional", "fr1", "fr2", "en", "ja", "neutral"]
    );
    let mut no_neutral = config;
    no_neutral.include_neutral = Some(false);
    assert_eq!(no_neutral.select_images(images).len(), 5);
}

#[test]
fn oversized_image_response_preserves_roles_and_languages_within_host_limits() {
    let config = ProviderConfig {
        language: Some("fr".into()),
        fallback_languages: vec!["en".into()],
        ..Default::default()
    };
    let roles = [
        ImageType::Poster,
        ImageType::Backdrop,
        ImageType::Logo,
        ImageType::ClearArt,
        ImageType::Thumb,
        ImageType::Banner,
        ImageType::DiscArt,
    ];
    for padding in [0, 2048] {
        let mut images = Vec::new();
        for role in roles {
            for language in [Some("fr"), Some("en"), None] {
                for i in 0..50 {
                    images.push(MediaImage {
                        url: format!(
                            "https://example.test/{role:?}/{language:?}/{i}/{}",
                            "x".repeat(padding)
                        ),
                        image_type: role,
                        language: language.map(str::to_owned),
                        width: None,
                        height: None,
                        rating: None,
                        vote_count: None,
                        is_primary: false,
                    });
                }
            }
        }
        let result = config.select_images(images);
        assert!(result.len() <= 200);
        assert!(
            serde_json::to_vec(&PluginResponse::ok(&result))
                .unwrap()
                .len()
                < 64 * 1024
        );
        for role in roles {
            for language in [Some("fr"), Some("en"), None] {
                assert!(
                    result
                        .iter()
                        .any(|image| image.image_type == role
                            && image.language.as_deref() == language),
                    "missing {role:?}/{language:?}"
                );
            }
        }
        if padding == 0 {
            for role in roles {
                assert!(
                    result
                        .iter()
                        .filter(|image| image.image_type == role)
                        .count()
                        > 3
                );
            }
        }
    }
}

#[tokio::test]
async fn handle_extension_rejects_unsupported_api_version() {
    let req = aether_plugin_api::ExtensionRequest {
        api_version: 99,
        operation: "manifest".into(),
        input: serde_json::Value::Null,
    };
    let res = aether_plugin_api::http::handle_extension(&MockPlugin, req);
    assert_eq!(res.api_version, API_VERSION);
    assert_eq!(res.result["error"], "unsupported_api_version");
}

#[tokio::test]
async fn handle_extension_proposes_metadata_when_unique() {
    let req = aether_plugin_api::ExtensionRequest {
        api_version: 1,
        operation: "metadata.propose".into(),
        input: serde_json::json!({
            "title": "Interstellar",
            "year": 2014,
            "mediaType": "movie"
        }),
    };
    let res = aether_plugin_api::http::handle_extension(&MockPlugin, req);
    assert_eq!(res.api_version, API_VERSION);
    assert_eq!(res.result["title"], "Interstellar");
    assert_eq!(res.result["summary"], "Mock overview");
}

#[test]
fn music_artist_is_emitted_as_a_structured_field() {
    let mut response = aether_plugin_api::ExtensionResponse {
        api_version: 1,
        result: serde_json::json!([
            {"id":"1","title":"Album","mediaType":"music","overview":"Artist"},
            {"id":"2","title":"Unknown","mediaType":"music"},
            {"id":"3","title":"Film","mediaType":"movie","overview":"Synopsis"}
        ]),
    };
    aether_plugin_api::http::promote_music_artist(&mut response);
    assert_eq!(response.result[0]["artist"], "Artist");
    assert!(response.result[1].get("artist").is_none());
    assert!(response.result[2].get("artist").is_none());
}
