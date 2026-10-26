use aether_plugin_intro_skipper::{
    ChapterClassifier, ChapterItem, DetectionMethod, EpisodeData, FingerprintMatcher,
    IntroSkipperConfig, IntroSkipperPlugin, MarkerType, SeasonAnalyzer,
    handle_intro_skipper_request,
};

#[test]
fn compact_host_fingerprints_detect_intro_without_false_credits() {
    use base64::Engine;
    let theme: Vec<u32> = (0..300_u32).map(|i| i.wrapping_mul(0x1f2e3d4c)).collect();
    let encode = |prefix: u32| {
        let words: Vec<u32> = (0..100)
            .map(|i| i ^ prefix)
            .chain(theme.iter().copied())
            .collect();
        let bytes: Vec<u8> = words.into_iter().flat_map(u32::to_le_bytes).collect();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    };
    let req = media_extension_sdk::ExtensionRequest {
        api_version: 1,
        operation: "intro.detect".into(),
        input: serde_json::json!({"episodes":[
            {"id":"a","seriesId":"series","seasonNumber":1,"duration":1200.0,"fingerprintRate":10.0,"fingerprintsCompact":encode(0x12345678)},
            {"id":"b","seriesId":"series","seasonNumber":1,"duration":1200.0,"fingerprintRate":10.0,"fingerprintsCompact":encode(0xabcdef01)}
        ]}),
    };
    let response = handle_intro_skipper_request(&IntroSkipperPlugin::new(), req);
    let episodes = response.result.as_array().unwrap();
    assert!(episodes.iter().all(|ep| {
        ep["markers"]
            .as_array()
            .is_some_and(|markers| markers.iter().any(|m| m["markerType"] == "intro"))
    }));
    assert!(episodes.iter().all(|ep| {
        ep["markers"]
            .as_array()
            .is_some_and(|markers| markers.iter().all(|m| m["markerType"] != "credits"))
    }));
}

#[test]
fn test_plugin_manifest_structure() {
    let plugin = IntroSkipperPlugin::new();
    let manifest = plugin.manifest();

    assert_eq!(manifest.api_version, 1);
    assert_eq!(manifest.id, "intro-skipper");
    assert_eq!(manifest.category, "Playback");
    assert!(
        manifest.http_hosts.is_empty(),
        "Intro Skipper runs 100% locally with zero external network requirement"
    );
    assert!(manifest.config_schema.len() >= 6);
    assert!(
        manifest
            .config_schema
            .iter()
            .any(|f| f.key == "min_intro_duration")
    );
    assert!(
        manifest
            .config_schema
            .iter()
            .any(|f| f.key == "similarity_threshold")
    );
    assert!(
        manifest
            .config_schema
            .iter()
            .any(|f| f.key == "auto_skip_intro")
    );
}

#[test]
fn never_compare_different_seasons_or_series() {
    let config = IntroSkipperConfig::default();
    let analyzer = SeasonAnalyzer::new(&config);
    let fingerprint: Vec<u32> = (0_u32..300).map(|i| i ^ i.rotate_left(7)).collect();
    let episode = |id: &str, series: &str, season| EpisodeData {
        id: id.into(),
        series_id: Some(series.into()),
        season_number: Some(season),
        duration: 1800.0,
        fingerprints: Some(fingerprint.clone()),
        fingerprint_rate: Some(10.0),
        ..Default::default()
    };
    for other in [episode("b", "show-a", 2), episode("b", "show-b", 1)] {
        let results = analyzer.analyze_season(&[episode("a", "show-a", 1), other]);
        assert!(results.iter().all(|r| r.markers.is_empty()));
    }
}

#[test]
fn silence_does_not_create_a_fingerprint_marker() {
    let config = IntroSkipperConfig::default();
    let matcher = FingerprintMatcher::new(&config, Some(10.0));
    assert!(
        matcher
            .find_best_match(&vec![0; 500], &vec![0; 500])
            .is_none()
    );
}

#[test]
fn flat_host_settings_change_chapter_detection() {
    let req = media_extension_sdk::ExtensionRequest {
        api_version: 1,
        operation: "intro.parse_chapters".into(),
        input: serde_json::json!({"episodeId":"ep","duration":120.0,"chapters":[{"start":0.0,"end":30.0,"title":"Intro"}],"config":{"min_intro_duration":40}}),
    };
    let response = handle_intro_skipper_request(&IntroSkipperPlugin::new(), req);
    assert_eq!(response.result, serde_json::json!([]));
}

#[test]
fn test_chapter_detection_multilingual() {
    let config = IntroSkipperConfig::default();
    let classifier = ChapterClassifier::new(&config);

    let chapters = vec![
        ChapterItem {
            start: 0.0,
            end: Some(85.0),
            title: "Prologue".into(),
        },
        ChapterItem {
            start: 85.0,
            end: Some(175.0),
            title: "Générique d'ouverture".into(),
        },
        ChapterItem {
            start: 175.0,
            end: Some(1350.0),
            title: "Épisode".into(),
        },
        ChapterItem {
            start: 1350.0,
            end: Some(1440.0),
            title: "Générique de fin".into(),
        },
    ];

    let markers = classifier.parse_markers("ep-1", &chapters, 1440.0);
    assert_eq!(markers.len(), 2);

    let intro = markers
        .iter()
        .find(|m| m.marker_type == MarkerType::Intro)
        .unwrap();
    assert_eq!(intro.start, 85.0);
    assert_eq!(intro.end, 175.0);
    assert_eq!(intro.duration(), 90.0);
    assert_eq!(intro.method, DetectionMethod::Chapter);

    let credits = markers
        .iter()
        .find(|m| m.marker_type == MarkerType::Credits)
        .unwrap();
    assert_eq!(credits.start, 1350.0);
    assert_eq!(credits.end, 1440.0);
    assert_eq!(credits.duration(), 90.0);
    assert_eq!(credits.method, DetectionMethod::Chapter);
}

#[test]
fn test_chapter_detection_rejection_of_invalid_durations() {
    let config = IntroSkipperConfig::default();
    let classifier = ChapterClassifier::new(&config);

    // Chapter named "Intro" but lasting 45 minutes (2700s) should be rejected
    let chapters = vec![ChapterItem {
        start: 0.0,
        end: Some(2700.0),
        title: "Intro".into(),
    }];
    let markers = classifier.parse_markers("ep-long", &chapters, 3000.0);
    assert!(
        markers.is_empty(),
        "Abnormally long intro chapter must be rejected"
    );
}

#[test]
fn test_fingerprint_matching_exact() {
    let config = IntroSkipperConfig::default();
    let rate = 10.0; // 10 frames per second
    let matcher = FingerprintMatcher::new(&config, Some(rate));

    // Create 30 seconds of identical intro theme (300 frames)
    let intro_theme: Vec<u32> = (0..300)
        .map(|i| 0x12345678 ^ ((i as u32).wrapping_mul(0x9e3779b9)))
        .collect();

    // Episode A: 10s cold open (100 frames) + 30s intro + 50s show
    let mut ep_a = vec![0xAAAAAAAA; 100];
    ep_a.extend_from_slice(&intro_theme);
    ep_a.extend(vec![0x55555555; 500]);

    // Episode B: 0s cold open + 30s intro + 60s show
    let mut ep_b = intro_theme.clone();
    ep_b.extend(vec![0x33333333; 600]);

    let matched = matcher.find_best_match(&ep_a, &ep_b);
    assert!(matched.is_some(), "Should detect matching intro segment");

    let m = matched.unwrap();
    // Offset A should be ~10s (cold open)
    assert!(
        (m.offset_a - 10.0).abs() < 1.0,
        "Expected offset_a around 10.0s, got {}",
        m.offset_a
    );
    // Offset B should be ~0s
    assert!(
        m.offset_b < 1.0,
        "Expected offset_b around 0.0s, got {}",
        m.offset_b
    );
    // Duration should be ~30s
    assert!(
        (m.duration - 30.0).abs() < 1.5,
        "Expected duration around 30.0s, got {}",
        m.duration
    );
    assert!(
        m.score >= 0.95,
        "Expected near-perfect score, got {}",
        m.score
    );
}

#[test]
fn test_fingerprint_matching_with_noise() {
    let config = IntroSkipperConfig::default();
    let rate = 10.0;
    let matcher = FingerprintMatcher::new(&config, Some(rate));

    // Base theme (40 seconds = 400 frames)
    let base_theme: Vec<u32> = (0..400)
        .map(|i| 0x0F0F0F0F ^ ((i as u32).wrapping_mul(0x1F2E3D4C)))
        .collect();

    // Episode A has the clean theme at offset 20s
    let mut ep_a = vec![0xAAAAAAAA; 200];
    ep_a.extend_from_slice(&base_theme);
    ep_a.extend(vec![0x55555555; 400]);

    // Episode B has the theme with 1 or 2 flipped bits per frame (compression artifacts) at offset 5s
    let mut noisy_theme = base_theme.clone();
    for (i, val) in noisy_theme.iter_mut().enumerate() {
        *val ^= 1 << (i % 32); // Flip exactly 1 bit out of 32 (similarity = 31/32 = 0.96875)
    }

    let mut ep_b = vec![0x33333333; 50];
    ep_b.extend_from_slice(&noisy_theme);
    ep_b.extend(vec![0xCCCCCCCC; 500]);

    let matched = matcher.find_best_match(&ep_a, &ep_b);
    assert!(
        matched.is_some(),
        "Should detect matching intro even with bit noise"
    );

    let m = matched.unwrap();
    assert!((m.offset_a - 20.0).abs() < 1.5);
    assert!((m.offset_b - 5.0).abs() < 1.5);
    assert!((m.duration - 40.0).abs() < 2.0);
    assert!(m.score >= 0.95);
    assert!((m.offset_b - 5.0).abs() < 1.5);
    assert!((m.duration - 40.0).abs() < 2.0);
    assert!(m.score >= 0.95);
}

#[test]
fn test_season_consensus_analysis() {
    let config = IntroSkipperConfig::default();
    let analyzer = SeasonAnalyzer::new(&config);

    let intro_theme: Vec<u32> = (0..250)
        .map(|i| 0xCAFEBABE ^ ((i as u32).wrapping_mul(0x718293a4)))
        .collect();

    // 3 episodes in season
    // Ep 1: 15s cold open + 25s intro
    let mut ep1_fps = vec![0x11111111; 150];
    ep1_fps.extend_from_slice(&intro_theme);
    ep1_fps.extend(vec![0x22222222; 800]);

    // Ep 2: 0s cold open + 25s intro
    let mut ep2_fps = intro_theme.clone();
    ep2_fps.extend(vec![0x33333333; 900]);

    // Ep 3: Has chapter metadata for intro
    let ep3_chapters = vec![
        ChapterItem {
            start: 0.0,
            end: Some(45.0),
            title: "Prologue".into(),
        },
        ChapterItem {
            start: 45.0,
            end: Some(70.0),
            title: "Opening".into(),
        },
        ChapterItem {
            start: 70.0,
            end: Some(1200.0),
            title: "Episode".into(),
        },
    ];

    let episodes = vec![
        EpisodeData {
            id: "ep-1".into(),
            series_id: Some("series-1".into()),
            season_number: Some(1),
            duration: 1200.0,
            fingerprints: Some(ep1_fps),
            fingerprint_rate: Some(10.0),
            chapters: None,
            ..Default::default()
        },
        EpisodeData {
            id: "ep-2".into(),
            series_id: Some("series-1".into()),
            season_number: Some(1),
            duration: 1200.0,
            fingerprints: Some(ep2_fps),
            fingerprint_rate: Some(10.0),
            chapters: None,
            ..Default::default()
        },
        EpisodeData {
            id: "ep-3".into(),
            series_id: Some("series-1".into()),
            season_number: Some(1),
            duration: 1200.0,
            fingerprints: None,
            fingerprint_rate: None,
            chapters: Some(ep3_chapters),
            ..Default::default()
        },
    ];

    let results = analyzer.analyze_season(&episodes);
    assert_eq!(results.len(), 3);

    // Ep 1 intro detected via fingerprint
    assert!(results[0].intro.is_some());
    let ep1_intro = results[0].intro.as_ref().unwrap();
    assert_eq!(ep1_intro.method, DetectionMethod::Fingerprint);
    assert!((ep1_intro.start - 15.0).abs() < 1.5);

    // Ep 2 intro detected via fingerprint
    assert!(results[1].intro.is_some());
    let ep2_intro = results[1].intro.as_ref().unwrap();
    assert_eq!(ep2_intro.method, DetectionMethod::Fingerprint);
    assert!(ep2_intro.start < 1.5);

    // Ep 3 intro detected via chapter
    assert!(results[2].intro.is_some());
    let ep3_intro = results[2].intro.as_ref().unwrap();
    assert_eq!(ep3_intro.method, DetectionMethod::Chapter);
    assert_eq!(ep3_intro.start, 45.0);
    assert_eq!(ep3_intro.end, 70.0);
}

#[test]
fn test_ipc_dispatch_operations() {
    let plugin = IntroSkipperPlugin::new();

    // 1. Manifest
    let req = media_extension_sdk::ExtensionRequest {
        api_version: 1,
        operation: "manifest".into(),
        input: serde_json::json!({}),
    };
    let res = handle_intro_skipper_request(&plugin, req);
    assert_eq!(res.api_version, 1);
    assert_eq!(res.result["id"], "intro-skipper");

    // 2. Parse chapters
    let req = media_extension_sdk::ExtensionRequest {
        api_version: 1,
        operation: "intro.parse_chapters".into(),
        input: serde_json::json!({
            "episodeId": "ep-42",
            "duration": 1400.0,
            "chapters": [
                { "start": 100.0, "end": 190.0, "title": "Opening Theme" }
            ]
        }),
    };
    let res = handle_intro_skipper_request(&plugin, req);
    assert!(res.result.is_array());
    let markers = res.result.as_array().unwrap();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0]["start"], 100.0);
    assert_eq!(markers[0]["end"], 190.0);
    assert_eq!(markers[0]["markerType"], "intro");
}
