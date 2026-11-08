//! Chapter names in the forms real releases use, the shape of unnamed anime chapters, film
//! credits, and how acoustic segments are fused with chapters.
use aether_plugin_intro_skipper::adjust::{nearest, snap_segment};
use aether_plugin_intro_skipper::{
    ChapterClassifier, ChapterItem, DetectionMethod, EpisodeData, IntroSkipperConfig,
    IntroSkipperPlugin, MarkerType, MediaKind, SeasonAnalyzer, handle_intro_skipper_request,
};

fn chapters(list: &[(f64, &str)]) -> Vec<ChapterItem> {
    list.iter()
        .map(|(start, title)| ChapterItem {
            start: *start,
            end: None,
            title: (*title).into(),
        })
        .collect()
}

#[test]
fn release_style_chapter_names_are_recognised() {
    for title in [
        "OP",
        "OP1",
        "OP 2",
        "Opening 2",
        "01 - Opening",
        "Opening - Medley",
        "OP1: Unravel",
        "♪ Opening Theme",
        "Générique de début",
        "Vorspann",
        "Abertura",
        "オープニング",
        "Main Titles",
        "Intro",
    ] {
        assert!(ChapterClassifier::is_intro_title(title), "{title}");
    }
    for title in [
        "ED",
        "ED2",
        "ED v2",
        "Ending 1",
        "End Credits",
        "Closing Credits",
        "Staff Roll",
        "Générique de fin",
        "Abspann",
        "Créditos finales",
        "エンディング",
        "05. Credits",
    ] {
        assert!(ChapterClassifier::is_credits_title(title), "{title}");
    }
    for title in [
        "Previously on Dark",
        "Recap",
        "Résumé",
        "前回のあらすじ",
        "The Story So Far",
    ] {
        assert!(ChapterClassifier::is_recap_title(title), "{title}");
    }
    for title in [
        "Next Episode Preview",
        "Preview",
        "次回予告",
        "Jikai Yokoku",
        "Prochain épisode",
    ] {
        assert!(ChapterClassifier::is_preview_title(title), "{title}");
    }
    // Story content that merely contains a keyword is never skipped.
    for title in [
        "Opening Night",
        "The Ending of Act I",
        "Post-credits scene",
        "Scène post-générique",
        "Introduction to the case",
        "Opening End",
        "Part A",
    ] {
        assert!(
            !ChapterClassifier::is_intro_title(title)
                && !ChapterClassifier::is_credits_title(title),
            "{title}"
        );
    }
}

#[test]
fn consecutive_chapters_of_one_kind_form_one_segment() {
    let config = IntroSkipperConfig::default();
    let markers = ChapterClassifier::new(&config).parse_markers(
        "ep",
        &chapters(&[
            (0.0, "Prologue"),
            (60.0, "Story"),
            (1250.0, "Ending"),
            (1310.0, "Credits"),
            (1400.0, "Story"),
        ]),
        1440.0,
    );
    let credits: Vec<_> = markers
        .iter()
        .filter(|m| m.marker_type == MarkerType::Credits)
        .collect();
    assert_eq!(credits.len(), 1);
    assert_eq!((credits[0].start, credits[0].end), (1250.0, 1400.0));
}

#[test]
fn unnamed_anime_chapters_reveal_opening_ending_and_preview() {
    let config = IntroSkipperConfig::default();
    let classifier = ChapterClassifier::new(&config);
    // Avant 0-88 (as long as a song), OP 88-178 (90 s), A, B, ED 1290-1380 (90 s), preview.
    let list = chapters(&[
        (0.0, "Chapter 01"),
        (88.0, "Chapter 02"),
        (178.0, "Chapter 03"),
        (720.0, "Chapter 04"),
        (1290.0, "Chapter 05"),
        (1380.0, "Chapter 06"),
    ]);
    let markers = classifier.parse_markers("ep", &list, 1410.0);
    let find = |kind| markers.iter().find(|m| m.marker_type == kind).unwrap();
    assert_eq!(
        (find(MarkerType::Intro).start, find(MarkerType::Intro).end),
        (88.0, 178.0)
    );
    assert_eq!(
        (
            find(MarkerType::Credits).start,
            find(MarkerType::Credits).end
        ),
        (1290.0, 1380.0)
    );
    assert_eq!(find(MarkerType::Preview).start, 1380.0);
    // Both found: each confirms the other and clears the default playback floor (0.80).
    assert!(find(MarkerType::Intro).confidence >= 0.85);
    assert_eq!(find(MarkerType::Intro).method, DetectionMethod::Chapter);

    // Scene titles (a film, a drama) are not guessed from.
    let titled = chapters(&[
        (0.0, "The Heist"),
        (95.0, "Night Train"),
        (185.0, "Betrayal"),
        (1290.0, "The Bridge"),
        (1380.0, "Dawn"),
    ]);
    assert!(classifier.parse_markers("ep", &titled, 1410.0).is_empty());

    // The guess can be switched off.
    let off = IntroSkipperConfig {
        enable_chapter_structure: false,
        ..Default::default()
    };
    assert!(
        ChapterClassifier::new(&off)
            .parse_markers("ep", &list, 1410.0)
            .is_empty()
    );
}

#[test]
fn a_film_keeps_long_credits_and_is_never_guessed_from_structure() {
    let request = |media: &str| {
        handle_intro_skipper_request(
            &IntroSkipperPlugin::new(),
            media_extension_sdk::ExtensionRequest {
                api_version: 1,
                operation: "intro.parse_chapters".into(),
                input: serde_json::json!({
                    "episodeId": "film",
                    "duration": 7200.0,
                    "mediaType": media,
                    "chapters": [
                        {"start": 0.0, "title": "Chapter 1"},
                        {"start": 3600.0, "title": "Chapter 2"},
                        {"start": 6600.0, "title": "End Credits"}
                    ]
                }),
            },
        )
        .result
    };
    // Ten minutes of credits: too long for an episode, ordinary for a film.
    let film = request("movie");
    let markers = film.as_array().unwrap();
    assert_eq!(markers.len(), 1, "{film}");
    assert_eq!(markers[0]["markerType"], "credits");
    assert_eq!(markers[0]["start"], 6600.0);
    assert_eq!(request("episode"), serde_json::json!([]));
    assert_eq!(
        IntroSkipperPlugin::new()
            .manifest()
            .supported_media_types
            .len(),
        4
    );
    let _ = MediaKind::Movie;
}

#[test]
fn acoustic_boundaries_land_on_nearby_chapters_and_file_edges() {
    let marks = [0.0, 92.0, 182.5, 1300.0];
    assert_eq!(nearest(&marks, 181.0, 3.0), 182.5);
    assert_eq!(nearest(&marks, 170.0, 3.0), 170.0);
    // Opening found 1.6 s after the start, ending 1.8 s before a chapter mark.
    assert_eq!(
        snap_segment(MarkerType::Intro, 1.6, 180.7, 1420.0, &marks, 3.0, None),
        (0.0, 182.5)
    );
    // Credits stopping 3 s before the end of the file (Chromaprint's last item) end with it.
    assert_eq!(
        snap_segment(MarkerType::Credits, 1330.0, 1417.0, 1420.0, &[], 3.0, None),
        (1330.0, 1420.0)
    );
    // An opening cut at the edge of the analysed audio reaches the chapter just after it.
    assert_eq!(
        snap_segment(
            MarkerType::Intro,
            230.0,
            299.5,
            1420.0,
            &[0.0, 320.0],
            3.0,
            Some(300.0)
        ),
        (230.0, 320.0)
    );
    // A window of 0 never moves a boundary.
    assert_eq!(
        snap_segment(
            MarkerType::Intro,
            40.0,
            130.0,
            1420.0,
            &[41.0, 131.0],
            0.0,
            None
        ),
        (40.0, 130.0)
    );
}

fn theme_episode(id: &str, prefix: u32, chapters: Option<Vec<ChapterItem>>) -> EpisodeData {
    // 10 items per second: 30 s of episode-specific audio, then a 60 s shared theme.
    let theme: Vec<u32> = (0..600_u32).map(|i| i.wrapping_mul(0x1f2e_3d4c)).collect();
    let words: Vec<u32> = (0..300_u32)
        .map(|i| i.wrapping_mul(0x9e37_79b9) ^ prefix)
        .chain(theme)
        .chain((0..300_u32).map(|i| i.wrapping_mul(0x85eb_ca6b) ^ prefix))
        .collect();
    EpisodeData {
        id: id.into(),
        series_id: Some("show".into()),
        season_number: Some(1),
        duration: 1400.0,
        fingerprints: Some(words),
        fingerprint_rate: Some(10.0),
        chapters,
        ..Default::default()
    }
}

#[test]
fn named_chapter_is_kept_as_authored_and_confirmed_by_the_audio() {
    let config = IntroSkipperConfig::default();
    // The chapter says the opening runs 29-91; the audio finds about 30-90.
    let named = chapters(&[(0.0, "Prologue"), (29.0, "Opening"), (91.0, "Part A")]);
    let episodes = vec![
        theme_episode("a", 0x1111_1111, Some(named)),
        theme_episode("b", 0x2222_2222, None),
    ];
    let results = SeasonAnalyzer::new(&config).analyze_season(&episodes);
    let a = results[0].intro.as_ref().expect("intro a");
    assert_eq!((a.start, a.end), (29.0, 91.0));
    assert_eq!(a.method, DetectionMethod::Chapter);
    assert!(a.confidence >= 0.99);
    let b = results[1].intro.as_ref().expect("intro b");
    assert_eq!(b.method, DetectionMethod::Fingerprint);
    assert!(
        (b.start - 30.0).abs() < 2.0 && (b.end - 90.0).abs() < 2.0,
        "{b:?}"
    );
}
