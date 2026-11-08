//! End credits read from the picture: lettering on a black background at the end of a film.
//!
//! The Aether Node decodes the keyframes of the end of the file (the last 20 minutes) and sends
//! the runs of black keyframes it found, each with how many of its keyframes carry lettering
//! (bright detail over the black). This mirrors the keyframe analyzer of the Jellyfin
//! intro-skipper plugin: a black scene is credits when it lasts long enough and most of its
//! keyframes are lettered; a fade to black, a cut between acts or a night scene is not.
//!
//! The picture alone is enough for a film whose chapters say nothing, and it confirms a chapter
//! named only by an ambiguous word ("Fin", "Ending"). The credits start on the nearest chapter
//! mark when one is close, since a chapter is placed on the exact cut.
use crate::chapters::{AMBIGUOUS_NAME_CONFIDENCE, ChapterSpan, NAMED_CHAPTER_CONFIDENCE};
use crate::types::{DetectionMethod, IntroMarker, IntroSkipperConfig, MarkerType};
use serde::{Deserialize, Serialize};

/// The only measurement this module reads; another algorithm is ignored.
pub const ALGORITHM: &str = "blackframe-v1";
/// Lettering on black for at least this long is reported for playback.
pub const IMAGE_ALONE_MIN: f64 = 90.0;
/// A shorter lettered black scene (one or two cards) is only proposed, unless a chapter starts on it.
pub const FILM_CREDITS_MIN: f64 = 60.0;
/// Lettered black from the picture alone.
pub const IMAGE_ALONE_CONFIDENCE: f64 = 0.84;
/// Lettered black that is too short to be sure of on its own.
pub const IMAGE_SHORT_CONFIDENCE: f64 = 0.74;
/// Lettered black that starts on a chapter mark (any chapter, named or not).
pub const IMAGE_AND_CHAPTER_CONFIDENCE: f64 = 0.92;
/// A chapter mark this close to the start or the end of the black (seconds) is where it really is.
pub const CHAPTER_SNAP: f64 = 15.0;
/// Two black runs this close (seconds) are one credits sequence: a studio logo or a colour card
/// between two parts of the roll.
const MERGE_GAP: f64 = 12.0;
/// Share of lettered keyframes above which a black scene is a roll; a gap between acts has none.
const LETTERED_SHARE: f64 = 0.5;
/// A roll is read over several keyframes, never one.
const MIN_PAGES: u32 = 3;
/// Most runs one answer carries; the Node sends the latest ones.
const MAX_RUNS: usize = 256;

/// One run of black keyframes measured by the Node.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackRun {
    pub start: f64,
    pub end: f64,
    /// Keyframes in the run.
    pub pages: u32,
    /// Keyframes of the run with lettering on them.
    pub lettered: u32,
}

/// The Node's measurement of the end of a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackFrameScan {
    pub algorithm: String,
    #[serde(default)]
    pub window_start: f64,
    #[serde(default)]
    pub window_end: f64,
    #[serde(default)]
    pub runs: Vec<BlackRun>,
}

/// A credits sequence seen in the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PictureCredits {
    pub start: f64,
    pub end: f64,
    pub lettered_share: f64,
}

impl PictureCredits {
    pub fn duration(&self) -> f64 {
        self.end - self.start
    }
}

impl BlackFrameScan {
    /// The scan sent with a request, if it is one this module reads.
    pub fn from_input(input: &serde_json::Value) -> Option<Self> {
        let scan: Self = serde_json::from_value(input.get("blackFrames")?.clone()).ok()?;
        (scan.algorithm == ALGORITHM).then_some(scan)
    }

    /// The latest lettered black scene that can be a film's credits: at least
    /// [`FILM_CREDITS_MIN`] seconds (and the configured minimum), no longer than the longest
    /// film credits, ending in the last quarter of the file. Runs split by a short colour card
    /// are read as one.
    pub fn credits(&self, duration: f64, config: &IntroSkipperConfig) -> Option<PictureCredits> {
        if !(duration.is_finite() && duration > 0.0) {
            return None;
        }
        let mut runs: Vec<BlackRun> = self
            .runs
            .iter()
            .take(MAX_RUNS)
            .copied()
            .filter(|run| {
                run.start.is_finite()
                    && run.end.is_finite()
                    && run.start >= 0.0
                    && run.end > run.start
                    && run.lettered <= run.pages
            })
            .map(|run| BlackRun {
                end: run.end.min(duration),
                ..run
            })
            .filter(|run| run.end > run.start)
            .collect();
        runs.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut merged: Vec<BlackRun> = Vec::with_capacity(runs.len());
        for run in runs {
            match merged.last_mut() {
                Some(last) if run.start - last.end <= MERGE_GAP => {
                    last.end = last.end.max(run.end);
                    last.pages += run.pages;
                    last.lettered += run.lettered;
                }
                _ => merged.push(run),
            }
        }
        let shortest = FILM_CREDITS_MIN.max(config.min_credits_duration);
        merged.iter().rev().find_map(|run| {
            let share = f64::from(run.lettered) / f64::from(run.pages.max(1));
            let length = run.end - run.start;
            (run.pages >= MIN_PAGES
                && share > LETTERED_SHARE
                && length >= shortest
                && length <= config.max_movie_credits_duration
                && run.end >= duration * 0.75)
                .then_some(PictureCredits {
                    start: run.start,
                    end: run.end,
                    lettered_share: share,
                })
        })
    }
}

/// The chapter mark (a chapter start, or the end of the file) closest to `time`, within
/// [`CHAPTER_SNAP`] seconds.
fn nearest_mark(marks: &[f64], time: f64) -> Option<f64> {
    marks
        .iter()
        .copied()
        .filter(|mark| (mark - time).abs() <= CHAPTER_SNAP)
        .min_by(|a, b| (a - time).abs().total_cmp(&(b - time).abs()))
}

/// Combine the credits seen in the picture with the film's chapter markers.
///
/// - A clearly named credits chapter ("End Credits") is kept as authored.
/// - Otherwise the picture decides: its credits start on the nearest chapter start and end on the
///   nearest chapter mark (or the end of the file) when one is within [`CHAPTER_SNAP`] seconds.
/// - A credits chapter named only by an ambiguous word ("Fin", "Ending") near the picture's start
///   is confirmed by it and reported.
/// - Without lettering on black, nothing changes: a film is never guessed from its structure.
pub fn combine_with_chapters(
    episode_id: &str,
    markers: &mut Vec<IntroMarker>,
    picture: Option<PictureCredits>,
    spans: &[ChapterSpan],
    duration: f64,
) {
    let Some(picture) = picture else {
        return;
    };
    let existing = markers
        .iter()
        .position(|marker| marker.marker_type == MarkerType::Credits);
    if let Some(index) = existing
        && markers[index].confidence >= NAMED_CHAPTER_CONFIDENCE
    {
        return;
    }
    let starts: Vec<f64> = spans.iter().map(|span| span.start).collect();
    let mut ends: Vec<f64> = spans.iter().map(|span| span.end).collect();
    ends.push(duration);
    let snapped_start = nearest_mark(&starts, picture.start);
    let start = snapped_start.unwrap_or(picture.start);
    let end = nearest_mark(&ends, picture.end).unwrap_or(picture.end);
    if end - start < 1.0 {
        return;
    }
    let ambiguous_confirmed = existing.is_some_and(|index| {
        let chapter = &markers[index];
        chapter.confidence <= AMBIGUOUS_NAME_CONFIDENCE
            && ((chapter.start - picture.start).abs() <= CHAPTER_SNAP
                || snapped_start.is_some_and(|mark| (mark - chapter.start).abs() < 0.05))
    });
    let confidence = if ambiguous_confirmed || snapped_start.is_some() {
        IMAGE_AND_CHAPTER_CONFIDENCE
    } else if picture.duration() >= IMAGE_ALONE_MIN {
        IMAGE_ALONE_CONFIDENCE
    } else {
        IMAGE_SHORT_CONFIDENCE
    };
    let marker = IntroMarker {
        id: format!("{episode_id}-credits-black"),
        start,
        end,
        marker_type: MarkerType::Credits,
        confidence,
        method: DetectionMethod::BlackFrame,
    };
    match existing {
        // An ambiguous chapter far from the picture is a scene title ("Fin" as the finale): the
        // picture wins when it is surer.
        Some(index) if markers[index].confidence < confidence => markers[index] = marker,
        Some(_) => {}
        None => markers.push(marker),
    }
}

/// [`combine_with_chapters`] for a film, unless the administrator turned the picture off.
pub fn film_credits(
    config: &IntroSkipperConfig,
    episode_id: &str,
    markers: &mut Vec<IntroMarker>,
    scan: Option<&BlackFrameScan>,
    spans: &[ChapterSpan],
    duration: f64,
) {
    if !config.enable_black_frame_detection {
        return;
    }
    let picture = scan.and_then(|scan| scan.credits(duration, config));
    combine_with_chapters(episode_id, markers, picture, spans, duration);
    markers.sort_by(|a, b| a.start.total_cmp(&b.start));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chapters::{ChapterClassifier, MediaKind, chapter_spans};
    use crate::types::ChapterItem;

    const FILM: f64 = 7200.0;

    fn run(start: f64, end: f64, pages: u32, lettered: u32) -> BlackRun {
        BlackRun {
            start,
            end,
            pages,
            lettered,
        }
    }

    fn scan(runs: Vec<BlackRun>) -> BlackFrameScan {
        BlackFrameScan {
            algorithm: ALGORITHM.into(),
            window_start: FILM - 1200.0,
            window_end: FILM,
            runs,
        }
    }

    fn chapters(list: &[(f64, &str)]) -> Vec<ChapterItem> {
        list.iter()
            .map(|(start, title)| ChapterItem {
                start: *start,
                end: None,
                title: (*title).into(),
            })
            .collect()
    }

    /// The whole film path: chapters first, then the picture.
    fn detect(list: &[(f64, &str)], frames: Option<BlackFrameScan>) -> Vec<IntroMarker> {
        let config = IntroSkipperConfig::default();
        let items = chapters(list);
        let mut markers = ChapterClassifier::new(&config).parse_markers_for(
            "film",
            &items,
            FILM,
            MediaKind::Movie,
        );
        film_credits(
            &config,
            "film",
            &mut markers,
            frames.as_ref(),
            &chapter_spans(&items, FILM),
            FILM,
        );
        markers
    }

    fn credits(markers: &[IntroMarker]) -> Option<&IntroMarker> {
        markers
            .iter()
            .find(|m| m.marker_type == MarkerType::Credits)
    }

    const UNNAMED: &[(f64, &str)] = &[
        (0.0, "Chapter 01"),
        (2400.0, "Chapter 02"),
        (4800.0, "Chapter 03"),
        (6700.0, "Chapter 04"),
    ];

    #[test]
    fn lettering_on_black_gives_credits_to_a_film_without_named_chapters() {
        // Eight minutes of roll that start 3 s after the last chapter mark.
        let found = detect(UNNAMED, Some(scan(vec![run(6703.0, FILM, 160, 150)])));
        let marker = credits(&found).expect("credits");
        assert_eq!(marker.start, 6700.0, "snapped to the chapter");
        assert_eq!(marker.end, FILM);
        assert_eq!(marker.method, DetectionMethod::BlackFrame);
        assert!(marker.confidence >= 0.80, "{}", marker.confidence);
    }

    #[test]
    fn the_picture_alone_is_above_the_playback_floor() {
        let found = detect(&[], Some(scan(vec![run(6650.0, FILM, 110, 100)])));
        let marker = credits(&found).expect("credits");
        assert_eq!(marker.start, 6650.0);
        assert_eq!(marker.confidence, IMAGE_ALONE_CONFIDENCE);
        assert!(marker.confidence >= 0.80);
    }

    #[test]
    fn a_short_fade_to_black_is_not_credits() {
        // Six seconds of black, then the end: a fade.
        let fade = scan(vec![run(FILM - 6.0, FILM, 3, 0)]);
        assert!(credits(&detect(UNNAMED, Some(fade))).is_none());
        // A long black scene without lettering (a night scene, a black interlude) is not either.
        let dark = scan(vec![run(FILM - 300.0, FILM, 60, 4)]);
        assert!(credits(&detect(UNNAMED, Some(dark))).is_none());
        // Nor a lettered card shorter than a minute.
        let card = scan(vec![run(FILM - 40.0, FILM, 10, 10)]);
        assert!(credits(&detect(UNNAMED, Some(card))).is_none());
    }

    #[test]
    fn an_ambiguous_chapter_confirmed_by_the_picture_rises_above_the_floor() {
        let list = &[
            (0.0, "1. Opening"),
            (3000.0, "2. Story"),
            (6600.0, "3. Fin"),
        ];
        let without = detect(list, None);
        let proposal = credits(&without).expect("chapter proposal");
        assert_eq!(proposal.confidence, AMBIGUOUS_NAME_CONFIDENCE);
        let with = detect(list, Some(scan(vec![run(6605.0, FILM, 140, 120)])));
        let marker = credits(&with).expect("credits");
        assert_eq!(marker.start, 6600.0);
        assert_eq!(marker.confidence, IMAGE_AND_CHAPTER_CONFIDENCE);
    }

    #[test]
    fn a_clearly_named_chapter_is_kept_as_authored() {
        let list = &[(0.0, "Chapter 1"), (6600.0, "End Credits")];
        let found = detect(list, Some(scan(vec![run(6640.0, FILM, 140, 120)])));
        let marker = credits(&found).unwrap();
        assert_eq!(marker.start, 6600.0);
        assert_eq!(marker.method, DetectionMethod::Chapter);
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn credits_end_before_a_post_credits_scene() {
        // Roll, then a 90 s mid-credits scene in colour, then a short black end card.
        let runs = vec![run(6500.0, 7080.0, 120, 110), run(FILM - 20.0, FILM, 4, 1)];
        let found = detect(&[], Some(scan(runs)));
        let marker = credits(&found).unwrap();
        assert_eq!((marker.start, marker.end), (6500.0, 7080.0));
    }

    #[test]
    fn a_logo_between_two_parts_of_the_roll_does_not_split_it() {
        let runs = vec![run(6500.0, 6800.0, 60, 55), run(6806.0, FILM, 80, 70)];
        let picture = scan(runs)
            .credits(FILM, &IntroSkipperConfig::default())
            .unwrap();
        assert_eq!((picture.start, picture.end), (6500.0, FILM));
    }

    #[test]
    fn black_in_the_middle_of_the_film_is_ignored_and_other_algorithms_too() {
        let early = scan(vec![run(3000.0, 3200.0, 40, 40)]);
        assert!(credits(&detect(&[], Some(early))).is_none());
        let input = serde_json::json!({"blackFrames": {"algorithm": "other", "runs": []}});
        assert!(BlackFrameScan::from_input(&input).is_none());
        let input = serde_json::json!({"blackFrames": {"algorithm": ALGORITHM, "runs": [
            {"start": 1.0, "end": 2.0, "pages": 3, "lettered": 1}
        ]}});
        assert_eq!(BlackFrameScan::from_input(&input).unwrap().runs.len(), 1);
    }

    #[test]
    fn nothing_is_guessed_without_the_picture() {
        assert!(credits(&detect(UNNAMED, None)).is_none());
        let config = IntroSkipperConfig {
            enable_black_frame_detection: false,
            ..Default::default()
        };
        let mut markers = Vec::new();
        film_credits(
            &config,
            "film",
            &mut markers,
            Some(&scan(vec![run(6650.0, FILM, 110, 100)])),
            &[],
            FILM,
        );
        assert!(markers.is_empty());
    }
}
