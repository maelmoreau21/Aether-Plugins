use aether_plugin_intro_skipper::{
    ChapterClassifier, ChapterItem, CreditsDetector, DetectionMethod, EpisodeData,
    FingerprintMatcher, IntroSkipperConfig, IntroSkipperPlugin, MarkerType, SeasonAnalyzer,
    handle_intro_skipper_request,
};

#[test]
fn detection_settings_bound_search_and_chapter_credits() {
    let request = |operation: &str, input| {
        handle_intro_skipper_request(
            &IntroSkipperPlugin::new(),
            media_extension_sdk::ExtensionRequest {
                api_version: 1,
                operation: operation.into(),
                input,
            },
        )
        .result
    };
    let chapters = serde_json::json!({"episodeId":"a","duration":1200,"chapters":[{"title":"Credits","start":1000,"end":1100}],"config":{"max_credits_duration":"60"}});
    assert_eq!(
        request("intro.parse_chapters", chapters.clone()),
        serde_json::json!([])
    );
    let mut allowed = chapters.clone();
    allowed["config"]["max_credits_duration"] = serde_json::json!(120);
    assert_eq!(
        request("intro.parse_chapters", allowed.clone())
            .as_array()
            .unwrap()
            .len(),
        1
    );
    allowed["config"]["enable_chapter_detection"] = serde_json::json!(false);
    assert_eq!(
        request("intro.parse_chapters", allowed),
        serde_json::json!([])
    );

    let theme: Vec<u32> = (0..200_u32).map(|i| i.wrapping_mul(0x1f2e3d4c)).collect();
    let input = serde_json::json!({"a":theme,"b":theme,"rate":10,"config":{"min_intro_duration":10,"search_window_start":0,"search_window_end":40}});
    assert!(request("intro.match_fingerprints", input.clone()).is_object());
    let mut excluded = input;
    excluded["config"]["search_window_start"] = serde_json::json!(25);
    assert!(request("intro.match_fingerprints", excluded).is_null());

    for config in [
        serde_json::json!({"max_credits_duration":451}),
        serde_json::json!({"min_intro_duration":90,"max_intro_duration":30}),
        serde_json::json!({"search_window_start":100,"search_window_end":80}),
        serde_json::json!({"similarity_threshold":"NaN"}),
        serde_json::json!({"search_window_start":"not-a-number"}),
    ] {
        assert_eq!(
            request(
                "intro.detect",
                serde_json::json!({"episodes":[],"config":config})
            )["error"],
            "invalid_detection_config"
        );
    }
    let schema = IntroSkipperPlugin::new().manifest().config_schema;
    for key in ["max_credits_duration", "search_window_start"] {
        assert!(schema.iter().any(|field| field.key == key));
    }
}

#[test]
fn silent_search_window_rejected_even_with_varied_audio_outside_it() {
    let config = IntroSkipperConfig {
        min_intro_duration: 5.0,
        search_window_end: 10.0,
        ..Default::default()
    };
    let words: Vec<u32> = std::iter::repeat_n(0, 100)
        .chain((0..200_u32).map(|i| i.wrapping_mul(0x1f2e3d4c)))
        .collect();
    assert!(
        FingerprintMatcher::new(&config, Some(10.0))
            .find_best_match(&words, &words)
            .is_none()
    );
}

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
    assert_eq!(manifest.permissions, vec!["playback.markers".to_string()]);
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
            .any(|f| f.key == "detect_intro")
    );
}

#[test]
fn rejects_unsupported_abi_or_api_version() {
    let plugin = IntroSkipperPlugin::new();
    let req = media_extension_sdk::ExtensionRequest {
        api_version: 2,
        operation: "manifest".into(),
        input: serde_json::Value::Null,
    };
    let res = handle_intro_skipper_request(&plugin, req);
    assert_eq!(res.api_version, 1);
    assert_eq!(res.result["error"], "unsupported_api_version");
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

#[test]
fn test_preview_and_recap_chapter_detection() {
    let config = IntroSkipperConfig::default();
    let classifier = ChapterClassifier::new(&config);

    let chapters = vec![
        ChapterItem {
            start: 0.0,
            end: Some(25.0),
            title: "Previously on Aether".into(),
        },
        ChapterItem {
            start: 25.0,
            end: Some(115.0),
            title: "Opening".into(),
        },
        ChapterItem {
            start: 1300.0,
            end: Some(1390.0),
            title: "Closing".into(),
        },
        ChapterItem {
            start: 1390.0,
            end: Some(1405.0),
            title: "Next episode preview".into(),
        },
    ];

    let markers = classifier.parse_markers("ep-preview-test", &chapters, 1420.0);
    assert_eq!(markers.len(), 4);
    assert_eq!(markers[0].marker_type, MarkerType::Recap);
    assert_eq!(markers[1].marker_type, MarkerType::Intro);
    assert_eq!(markers[2].marker_type, MarkerType::Credits);
    assert_eq!(markers[3].marker_type, MarkerType::Preview);
    assert_eq!(markers[3].start, 1390.0);
    assert_eq!(markers[3].end, 1405.0);
}

#[test]
fn test_mid_season_opening_change() {
    let config = IntroSkipperConfig::default();
    let analyzer = SeasonAnalyzer::new(&config);

    // Helper to generate dynamic audio sequence (non-flat)
    let gen_seq = |seed: u32, len: usize| -> Vec<u32> {
        let mut val = seed;
        (0..len)
            .map(|_| {
                val = val.wrapping_mul(1664525).wrapping_add(1013904223);
                val ^ (val >> 11)
            })
            .collect()
    };

    // Opening A for episodes 1-2, Opening B for episodes 3-4
    let theme_a = gen_seq(0xA1B2C3D4, 450);
    let theme_b = gen_seq(0xDEADBEEF, 450);

    let mut ep1_fps = gen_seq(0x1001, 100); // 10s cold open
    ep1_fps.extend(theme_a.clone());
    ep1_fps.extend(gen_seq(0x1002, 500));

    let mut ep2_fps = gen_seq(0x2001, 200); // 20s cold open
    ep2_fps.extend(theme_a.clone());
    ep2_fps.extend(gen_seq(0x2002, 500));

    let mut ep3_fps = gen_seq(0x3001, 50); // 5s cold open
    ep3_fps.extend(theme_b.clone());
    ep3_fps.extend(gen_seq(0x3002, 500));

    let mut ep4_fps = gen_seq(0x4001, 150); // 15s cold open
    ep4_fps.extend(theme_b.clone());
    ep4_fps.extend(gen_seq(0x4002, 500));

    let episodes = vec![
        EpisodeData {
            id: "ep-01".into(),
            series_id: Some("anime-s1".into()),
            season_number: Some(1),
            duration: 1400.0,
            fingerprints: Some(ep1_fps),
            fingerprint_rate: Some(10.0),
            ..Default::default()
        },
        EpisodeData {
            id: "ep-02".into(),
            series_id: Some("anime-s1".into()),
            season_number: Some(1),
            duration: 1400.0,
            fingerprints: Some(ep2_fps),
            fingerprint_rate: Some(10.0),
            ..Default::default()
        },
        EpisodeData {
            id: "ep-03".into(),
            series_id: Some("anime-s1".into()),
            season_number: Some(1),
            duration: 1400.0,
            fingerprints: Some(ep3_fps),
            fingerprint_rate: Some(10.0),
            ..Default::default()
        },
        EpisodeData {
            id: "ep-04".into(),
            series_id: Some("anime-s1".into()),
            season_number: Some(1),
            duration: 1400.0,
            fingerprints: Some(ep4_fps),
            fingerprint_rate: Some(10.0),
            ..Default::default()
        },
    ];

    let results = analyzer.analyze_season(&episodes);
    assert_eq!(results.len(), 4);

    // Ep 1 & 2 detected with Opening A offsets
    assert!(results[0].intro.is_some());
    let intro1 = results[0].intro.as_ref().unwrap();
    assert!((intro1.start - 10.0).abs() < 1.0);
    assert!((intro1.duration() - 45.0).abs() < 1.0);

    assert!(results[1].intro.is_some());
    let intro2 = results[1].intro.as_ref().unwrap();
    assert!((intro2.start - 20.0).abs() < 1.0);
    assert!((intro2.duration() - 45.0).abs() < 1.0);

    // Ep 3 & 4 detected with Opening B offsets
    assert!(results[2].intro.is_some());
    let intro3 = results[2].intro.as_ref().unwrap();
    assert!((intro3.start - 5.0).abs() < 1.5);
    assert!((intro3.duration() - 45.0).abs() < 2.5);

    assert!(results[3].intro.is_some());
    let intro4 = results[3].intro.as_ref().unwrap();
    assert!((intro4.start - 15.0).abs() < 1.5);
    assert!((intro4.duration() - 45.0).abs() < 2.5);
}

#[test]
fn test_post_credits_scene_detection() {
    let config = IntroSkipperConfig::default();
    let detector = CreditsDetector::new(&config);

    let mut credits_theme = Vec::new();
    for i in 0..600 {
        credits_theme.push(0xCAFEF00D ^ (i as u32 * 41));
    }

    // Ep A: standard credits at the very end
    let ep_a = EpisodeData {
        id: "ep-a".into(),
        duration: 1200.0,
        tail_start: Some(1140.0),
        tail_fingerprints: Some(credits_theme.clone()),
        fingerprint_rate: Some(10.0),
        ..Default::default()
    };

    // Ep B: has a 40-second post-credits scene! (Credits end at 1160, episode ends at 1200)
    let mut ep_b_tail = credits_theme.clone();
    ep_b_tail.extend(vec![0x12345678; 400]); // 40 seconds of post-credits scene audio

    let ep_b = EpisodeData {
        id: "ep-b".into(),
        duration: 1200.0,
        tail_start: Some(1100.0),
        tail_fingerprints: Some(ep_b_tail),
        fingerprint_rate: Some(10.0),
        ..Default::default()
    };

    let result = detector.detect_tail_credits(&ep_a, &ep_b);
    assert!(result.is_some());
    let (marker_a, marker_b) = result.unwrap();

    assert_eq!(marker_a.marker_type, MarkerType::Credits);
    assert_eq!(marker_b.marker_type, MarkerType::Credits);

    // Ep B post-credits scene: marker ends around 1160, leaving 40s before 1200
    assert!((marker_b.start - 1100.0).abs() < 1.5);
    assert!((marker_b.end - 1160.0).abs() < 1.5);
    assert!(
        ep_b.duration - marker_b.end >= 35.0,
        "Post-credits scene preserved!"
    );
}

#[test]
fn fingerprint_rate_is_the_chromaprint_rate_and_the_legacy_mislabel_is_corrected() {
    use aether_plugin_intro_skipper::{
        DEFAULT_FINGERPRINT_RATE, LEGACY_FINGERPRINT_RATE, normalize_rate,
    };
    // 11025 Hz audio, one item every 1365 samples.
    assert!((DEFAULT_FINGERPRINT_RATE - 8.076923).abs() < 1e-5);
    assert_eq!(normalize_rate(None), DEFAULT_FINGERPRINT_RATE);
    assert_eq!(normalize_rate(Some(f64::NAN)), DEFAULT_FINGERPRINT_RATE);
    assert_eq!(
        normalize_rate(Some(f64::INFINITY)),
        DEFAULT_FINGERPRINT_RATE
    );
    // What Aether hosts announced before 7 November 2026.
    assert_eq!(
        normalize_rate(Some(LEGACY_FINGERPRINT_RATE)),
        DEFAULT_FINGERPRINT_RATE
    );
    // Any other announced rate is the host's own business.
    assert_eq!(normalize_rate(Some(10.0)), 10.0);

    // A theme of 60 s at the real rate is reported as 60 s whichever label the host used.
    let noise = |seed: u32, n: usize| {
        let mut x = seed;
        (0..n)
            .map(|_| {
                x = x.wrapping_mul(1664525).wrapping_add(1013904223);
                x ^ (x >> 13)
            })
            .collect::<Vec<u32>>()
    };
    let theme = noise(0xA5A5_1234, 485);

    let (mut a, mut b) = (noise(7, 100), noise(99, 300));
    a.extend(&theme);
    b.extend(&theme);
    let config = aether_plugin_intro_skipper::IntroSkipperConfig::default();
    for rate in [
        Some(DEFAULT_FINGERPRINT_RATE),
        Some(LEGACY_FINGERPRINT_RATE),
        None,
    ] {
        let m = aether_plugin_intro_skipper::FingerprintMatcher::new(&config, rate)
            .find_best_match(&a, &b)
            .expect("match");
        assert!(
            (m.offset_a - 100.0 / DEFAULT_FINGERPRINT_RATE).abs() < 1.0,
            "{m:?}"
        );
        assert!(
            (m.offset_b - 300.0 / DEFAULT_FINGERPRINT_RATE).abs() < 1.0,
            "{m:?}"
        );
        assert!(
            (m.duration - 485.0 / DEFAULT_FINGERPRINT_RATE).abs() < 1.5,
            "{m:?}"
        );
    }
}

fn parse_chapters(config: serde_json::Value) -> serde_json::Value {
    handle_intro_skipper_request(
        &IntroSkipperPlugin::new(),
        media_extension_sdk::ExtensionRequest {
            api_version: 1,
            operation: "intro.parse_chapters".into(),
            input: serde_json::json!({
                "episodeId": "ep",
                "duration": 1500.0,
                "chapters": [
                    {"title": "Previously", "start": 0.0, "end": 40.0},
                    {"title": "Opening - Medley", "start": 40.0, "end": 130.0},
                    {"title": "Part A", "start": 130.0, "end": 1380.0},
                    {"title": "Rideau", "start": 1380.0, "end": 1470.0},
                    {"title": "Next Time", "start": 1470.0, "end": 1500.0}
                ],
                "config": config
            }),
        },
    )
    .result
}

fn kinds(result: &serde_json::Value) -> Vec<String> {
    result
        .as_array()
        .expect("markers")
        .iter()
        .map(|m| m["markerType"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn custom_chapter_keywords_extend_builtin_names() {
    // "Rideau" is not a built-in credits name; "Opening - Medley" starts with one.
    assert_eq!(
        kinds(&parse_chapters(serde_json::json!({}))),
        ["recap", "intro", "preview"]
    );
    let result = parse_chapters(serde_json::json!({"credits_keywords": "rideau, Générique"}));
    assert_eq!(kinds(&result), ["recap", "intro", "credits", "preview"]);
}

#[test]
fn disabled_kinds_are_not_reported() {
    let result = parse_chapters(serde_json::json!({
        "detect_recap": "false",
        "detect_preview": false,
        "credits_keywords": "rideau"
    }));
    assert_eq!(kinds(&result), ["intro", "credits"]);
}

#[test]
fn boundary_offsets_shift_and_clamp_segments() {
    let result = parse_chapters(serde_json::json!({
        "intro_start_offset": "2",
        "intro_end_offset": "-3.5",
        "credits_start_offset": 10,
        "credits_keywords": "rideau"
    }));
    let markers = result.as_array().unwrap();
    let intro = markers.iter().find(|m| m["markerType"] == "intro").unwrap();
    assert_eq!(
        (intro["start"].as_f64(), intro["end"].as_f64()),
        (Some(42.0), Some(126.5))
    );
    let credits = markers
        .iter()
        .find(|m| m["markerType"] == "credits")
        .unwrap();
    assert_eq!(
        (credits["start"].as_f64(), credits["end"].as_f64()),
        (Some(1390.0), Some(1470.0))
    );
    // Shifted boundaries are clamped to the episode.
    let config = IntroSkipperConfig {
        intro_start_offset: -30.0,
        ..Default::default()
    };
    let shifted = config.finalize(
        vec![aether_plugin_intro_skipper::IntroMarker {
            id: "x".into(),
            start: 5.0,
            end: 40.0,
            marker_type: MarkerType::Intro,
            confidence: 0.9,
            method: DetectionMethod::Chapter,
        }],
        1200.0,
    );
    assert_eq!((shifted[0].start, shifted[0].end), (0.0, 40.0));
}

#[test]
fn new_settings_are_validated() {
    for config in [
        serde_json::json!({"intro_end_offset": "31"}),
        serde_json::json!({"credits_start_offset": "abc"}),
        serde_json::json!({"min_consensus_votes": "0"}),
        serde_json::json!({"min_consensus_votes": "1.5"}),
        serde_json::json!({"min_consensus_votes": "5"}),
    ] {
        assert_eq!(
            parse_chapters(config.clone()),
            serde_json::json!({"error": "invalid_detection_config"}),
            "{config}"
        );
    }
    // An empty form field keeps the default.
    assert!(
        parse_chapters(serde_json::json!({"intro_end_offset": "", "min_consensus_votes": "2"}))
            .is_array()
    );
}

#[test]
fn every_schema_key_is_read_by_the_configuration() {
    let schema = IntroSkipperPlugin::new().manifest().config_schema;
    let defaults = IntroSkipperConfig::default();
    for field in &schema {
        let mut cfg = aether_plugin_api::ProviderConfig::default();
        let probe = match field.field_type.as_str() {
            "boolean" => "false",
            "text" => "probe keyword",
            "select" => "2",
            _ => "7",
        };
        cfg.extra.insert(field.key.clone(), probe.into());
        assert_ne!(
            IntroSkipperConfig::from_provider_config(&cfg),
            defaults,
            "schema key {} is ignored by the plugin",
            field.key
        );
    }
    assert!(schema.iter().all(|f| !f.key.starts_with("auto_skip")));
}
