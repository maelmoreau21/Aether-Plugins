//! Container chapters: named chapters ("Opening", "OP1", "Générique de fin", "エンディング"…)
//! give exact, authored segments; unnamed chapters ("Chapter 03", "00:21:30.000") of a typical
//! anime release still betray the opening and ending by their length and position.
use crate::types::{ChapterItem, DetectionMethod, IntroMarker, IntroSkipperConfig, MarkerType};

/// Confidence of a segment read from a chapter whose name says what it is.
pub const NAMED_CHAPTER_CONFIDENCE: f64 = 0.98;
/// Unnamed chapters shaped like an opening AND an ending (each confirms the other).
pub const STRUCTURE_CONFIRMED_CONFIDENCE: f64 = 0.86;
/// A single unnamed chapter shaped like an opening or an ending: shown to the administrator as
/// a proposal, below the default playback floor (80 %) unless the audio agrees.
pub const STRUCTURE_ALONE_CONFIDENCE: f64 = 0.72;

/// Length of a TV-size anime song, the shape the structural guess looks for (seconds).
const SONG_MIN: f64 = 75.0;
const SONG_MAX: f64 = 100.0;
/// Episodes longer than this are not guessed from structure (films, specials).
const STRUCTURE_MAX_DURATION: f64 = 75.0 * 60.0;

/// Kind of a chapter for one operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Episode,
    Movie,
}

/// Fast normalized title classification for chapter metadata
pub struct ChapterClassifier<'a> {
    config: &'a IntroSkipperConfig,
}

const INTRO_NAMES: &[&str] = &[
    "intro",
    "introduction",
    "intro start",
    "intro song",
    "intro theme",
    "intro sequence",
    "opening",
    "opening start",
    "opening theme",
    "opening song",
    "opening music",
    "opening credits",
    "opening titles",
    "opening title",
    "opening sequence",
    "opening animation",
    "op",
    "op theme",
    "op song",
    "nc op",
    "ncop",
    "title sequence",
    "titles",
    "main title",
    "main titles",
    "main title sequence",
    "theme song",
    "theme",
    "generique",
    "generique debut",
    "generique de debut",
    "generique d ouverture",
    "generique ouverture",
    "generique d intro",
    "ouverture",
    "vorspann",
    "abertura",
    "apertura",
    "cabecera",
    "sigla",
    "sigla iniziale",
    "sigla di apertura",
    "オープニング",
    "オープニングテーマ",
];

const CREDITS_NAMES: &[&str] = &[
    "credits",
    "credit",
    "the credits",
    "end credits",
    "end credit",
    "ending credits",
    "closing credits",
    "final credits",
    "end titles",
    "end title",
    "closing titles",
    "ending",
    "ending theme",
    "ending song",
    "ending music",
    "ending animation",
    "ed",
    "ed theme",
    "ed song",
    "nc ed",
    "nced",
    "outro",
    "closing",
    "closing theme",
    "staff roll",
    "staff credits",
    "cast list",
    "generique de fin",
    "generique fin",
    "generique final",
    "credits de fin",
    "fin",
    "abspann",
    "encerramento",
    "creditos",
    "creditos finales",
    "creditos finais",
    "titoli di coda",
    "sigla finale",
    "sigla di chiusura",
    "エンディング",
    "エンディングテーマ",
];

const RECAP_NAMES: &[&str] = &[
    "recap",
    "previously",
    "previously on",
    "previous episode",
    "precedemment",
    "resume",
    "rappel",
    "summary",
    "last time",
    "last time on",
    "last week",
    "last episode",
    "story so far",
    "the story so far",
    "catch up",
    "ruckblick",
    "was bisher geschah",
    "anteriormente",
    "previamente",
    "riassunto",
    "前回",
    "前回のあらすじ",
    "あらすじ",
];

const PREVIEW_NAMES: &[&str] = &[
    "preview",
    "previews",
    "next",
    "next episode",
    "next time",
    "next on",
    "next episode preview",
    "episode preview",
    "coming up",
    "coming soon",
    "sneak peek",
    "teaser",
    "tease",
    "pv",
    "prochain episode",
    "prochainement",
    "a suivre",
    "apercu",
    "bande annonce",
    "jikai",
    "jikai yokoku",
    "yokoku",
    "vorschau",
    "avance",
    "proximo episodio",
    "prossimo episodio",
    "prossimamente",
    "予告",
    "次回予告",
];

/// Phrases that open a longer name of the kind ("Previously on Dark", "Next time on…").
const RECAP_PREFIXES: &[&str] = &[
    "previously on ",
    "previously in ",
    "last time on ",
    "precedemment dans ",
    "dans l episode precedent",
    "en el episodio anterior",
    "nell episodio precedente",
];
const PREVIEW_PREFIXES: &[&str] = &[
    "next time on ",
    "next on ",
    "next episode ",
    "prochain episode",
    "dans le prochain episode",
    "a suivre dans",
];

/// Names that look like credits but are story content: never skipped.
const NOT_CREDITS: &[&str] = &[
    "post credit",
    "post credits",
    "mid credit",
    "mid credits",
    "after credits",
    "after the credits",
    "post generique",
    "apres le generique",
    "scene post",
    "stinger",
];

impl<'a> ChapterClassifier<'a> {
    pub fn new(config: &'a IntroSkipperConfig) -> Self {
        Self { config }
    }

    /// Check if chapter title matches common intro patterns
    pub fn is_intro_title(title: &str) -> bool {
        classify_builtin(title) == Some(MarkerType::Intro)
    }

    /// Check if chapter title matches common credits/outro patterns
    pub fn is_credits_title(title: &str) -> bool {
        classify_builtin(title) == Some(MarkerType::Credits)
    }

    /// Check if chapter title matches recap patterns
    pub fn is_recap_title(title: &str) -> bool {
        classify_builtin(title) == Some(MarkerType::Recap)
    }

    /// Check if chapter title matches preview/next episode patterns
    pub fn is_preview_title(title: &str) -> bool {
        classify_builtin(title) == Some(MarkerType::Preview)
    }

    /// Built-in names plus the administrator's own (`*_keywords` settings). A custom
    /// keyword matches the whole title or its first words ("opening" matches "Opening 2").
    pub fn matches(&self, kind: MarkerType, title: &str) -> bool {
        if classify_builtin(title) == Some(kind) {
            return true;
        }
        let custom = match kind {
            MarkerType::Intro => &self.config.intro_keywords,
            MarkerType::Credits => &self.config.credits_keywords,
            MarkerType::Recap => &self.config.recap_keywords,
            MarkerType::Preview => &self.config.preview_keywords,
        };
        if custom.is_empty() {
            return false;
        }
        let clean = clean_text(title);
        custom.iter().any(|keyword| {
            clean == *keyword
                || clean
                    .strip_prefix(keyword.as_str())
                    .is_some_and(|rest| rest.starts_with(' '))
        })
    }

    /// The kind a chapter name announces, custom keywords included.
    pub fn classify(&self, title: &str) -> Option<MarkerType> {
        [
            MarkerType::Intro,
            MarkerType::Credits,
            MarkerType::Recap,
            MarkerType::Preview,
        ]
        .into_iter()
        .find(|kind| self.matches(*kind, title))
    }

    fn bounds(&self, kind: MarkerType, media: MediaKind) -> (f64, f64) {
        match kind {
            MarkerType::Intro => (
                self.config.min_intro_duration,
                self.config.max_intro_duration,
            ),
            MarkerType::Credits => (
                self.config.min_credits_duration,
                if media == MediaKind::Movie {
                    self.config.max_movie_credits_duration
                } else {
                    self.config.max_credits_duration
                },
            ),
            MarkerType::Recap => (5.0, 180.0),
            MarkerType::Preview => (3.0, 120.0),
        }
    }

    /// Parse markers from an episode's chapter list
    pub fn parse_markers(
        &self,
        episode_id: &str,
        chapters: &[ChapterItem],
        episode_duration: f64,
    ) -> Vec<IntroMarker> {
        self.parse_markers_for(episode_id, chapters, episode_duration, MediaKind::Episode)
    }

    /// Named chapters first; consecutive chapters of the same kind ("Ending" then "Credits")
    /// form one segment. Without a named opening or ending, an episode's unnamed chapters are
    /// read for their shape (see [`Self::structural_markers`]).
    pub fn parse_markers_for(
        &self,
        episode_id: &str,
        chapters: &[ChapterItem],
        episode_duration: f64,
        media: MediaKind,
    ) -> Vec<IntroMarker> {
        let spans = chapter_spans(chapters, episode_duration);
        let mut markers: Vec<IntroMarker> = Vec::new();
        let mut i = 0;
        while i < spans.len() {
            let Some(kind) = self.classify(&spans[i].title) else {
                i += 1;
                continue;
            };
            let first = i;
            let mut end = spans[i].end;
            while i + 1 < spans.len() && self.classify(&spans[i + 1].title) == Some(kind) {
                i += 1;
                end = spans[i].end;
            }
            let start = spans[first].start;
            let (min, max) = self.bounds(kind, media);
            let length = end - start;
            // One segment of each kind: the opening is the first one, the credits the last.
            let already = markers.iter().position(|m| m.marker_type == kind);
            if length >= min && length <= max {
                let marker = IntroMarker {
                    id: format!("{episode_id}-{kind}-chap-{first}"),
                    start,
                    end,
                    marker_type: kind,
                    confidence: NAMED_CHAPTER_CONFIDENCE,
                    method: DetectionMethod::Chapter,
                };
                match (already, kind) {
                    (None, _) => markers.push(marker),
                    (Some(index), MarkerType::Credits | MarkerType::Preview) => {
                        markers[index] = marker
                    }
                    _ => {}
                }
            }
            i += 1;
        }

        if media == MediaKind::Episode && self.config.enable_chapter_structure {
            let has = |kind| markers.iter().any(|m: &IntroMarker| m.marker_type == kind);
            let (has_intro, has_credits, has_preview) = (
                has(MarkerType::Intro),
                has(MarkerType::Credits),
                has(MarkerType::Preview),
            );
            if !has_intro || !has_credits {
                for marker in self.structural_markers(episode_id, &spans, episode_duration) {
                    let wanted = match marker.marker_type {
                        MarkerType::Intro => !has_intro,
                        MarkerType::Credits => !has_credits,
                        MarkerType::Preview => !has_preview && !has_credits,
                        MarkerType::Recap => false,
                    };
                    if wanted {
                        markers.push(marker);
                    }
                }
            }
        }
        markers.sort_by(|a, b| a.start.total_cmp(&b.start));
        markers
    }

    /// Anime releases are chaptered "avant / opening / part A / part B / ending / preview" even
    /// when the chapters carry no name: the opening and the ending are the two chapters as long
    /// as a TV-size song (75–100 s), the first in the opening third, the second just before the
    /// end (or a short preview). Only unnamed chapters are read this way.
    pub fn structural_markers(
        &self,
        episode_id: &str,
        spans: &[ChapterSpan],
        duration: f64,
    ) -> Vec<IntroMarker> {
        if spans.len() < 3 || !(600.0..=STRUCTURE_MAX_DURATION).contains(&duration) {
            return Vec::new();
        }
        if spans.iter().any(|span| !is_generic_title(&span.title)) {
            return Vec::new();
        }
        let song = |span: &ChapterSpan| (SONG_MIN..=SONG_MAX).contains(&(span.end - span.start));
        // A cold open ("avant") can itself last as long as a song: when the first chapter and
        // another one both have the shape, the opening is the one after the cold open.
        let openings: Vec<usize> = (0..spans.len())
            .filter(|&i| song(&spans[i]) && spans[i].start <= (duration * 0.4).min(600.0))
            .collect();
        let opening = match openings.as_slice() {
            [0, second, ..] => Some(*second),
            [first, ..] => Some(*first),
            [] => None,
        };
        let ending = (0..spans.len()).rev().find(|&i| {
            let span = &spans[i];
            let tail_after = duration - span.end;
            Some(i) != opening
                && song(span)
                && span.start >= duration * 0.6
                && (tail_after <= 3.0
                    || (i + 1 == spans.len() - 1 && (5.0..=45.0).contains(&tail_after)))
        });
        let confidence = if opening.is_some() && ending.is_some() {
            STRUCTURE_CONFIRMED_CONFIDENCE
        } else {
            STRUCTURE_ALONE_CONFIDENCE
        };
        let mut markers = Vec::new();
        let mut push = |kind: MarkerType, index: usize, confidence: f64| {
            let span = &spans[index];
            let (min, max) = self.bounds(kind, MediaKind::Episode);
            if (min..=max).contains(&(span.end - span.start)) {
                markers.push(IntroMarker {
                    id: format!("{episode_id}-{kind}-shape-{index}"),
                    start: span.start,
                    end: span.end,
                    marker_type: kind,
                    confidence,
                    method: DetectionMethod::Chapter,
                });
            }
        };
        if let Some(index) = opening {
            push(MarkerType::Intro, index, confidence);
        }
        if let Some(index) = ending {
            push(MarkerType::Credits, index, confidence);
            if index + 1 == spans.len() - 1 {
                push(MarkerType::Preview, index + 1, STRUCTURE_ALONE_CONFIDENCE);
            }
        }
        markers
    }
}

/// One chapter with its resolved end (next chapter, or the end of the file).
#[derive(Clone, Debug, PartialEq)]
pub struct ChapterSpan {
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// Chapters sorted by start, each ending where the next begins (or at its own end / the end of
/// the file), clamped to the file. Empty and backwards chapters are dropped.
pub fn chapter_spans(chapters: &[ChapterItem], duration: f64) -> Vec<ChapterSpan> {
    let limit = if duration.is_finite() && duration > 0.0 {
        duration
    } else {
        f64::MAX
    };
    let mut sorted: Vec<&ChapterItem> = chapters
        .iter()
        .filter(|c| c.start.is_finite() && c.start >= 0.0 && c.start < limit)
        .collect();
    sorted.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut spans = Vec::with_capacity(sorted.len());
    for (i, chapter) in sorted.iter().enumerate() {
        let next = sorted.get(i + 1).map_or(limit, |next| next.start);
        // An explicit end is kept when it leaves a gap before the next chapter, and cut where
        // the next chapter starts when it overlaps it.
        let end = chapter
            .end
            .filter(|end| end.is_finite() && *end > chapter.start)
            .map_or(next, |end| end.min(next))
            .min(limit);
        if end > chapter.start {
            spans.push(ChapterSpan {
                start: chapter.start,
                end,
                title: chapter.title.clone(),
            });
        }
    }
    spans
}

/// Every chapter boundary (starts and ends) of a file, for snapping acoustic segments.
pub fn chapter_boundaries(chapters: &[ChapterItem], duration: f64) -> Vec<f64> {
    let mut points: Vec<f64> = chapter_spans(chapters, duration)
        .iter()
        .flat_map(|span| [span.start, span.end])
        .collect();
    points.sort_by(f64::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    points
}

/// "Chapter 3", "Chapitre 03", "00:21:30.000", "3", "", "Part A", "Avant", "Prologue": a name
/// that says nothing about whether the chapter can be skipped.
pub fn is_generic_title(title: &str) -> bool {
    let clean = clean_text(title);
    let words: Vec<&str> = clean.split(' ').filter(|w| !w.is_empty()).collect();
    if words.iter().all(|w| w.chars().all(|c| c.is_ascii_digit())) {
        return true;
    }
    let numbered = |w: &str| w.chars().all(|c| c.is_ascii_digit()) || (w.len() == 1);
    match words.as_slice() {
        [
            "chapter" | "chapitre" | "chap" | "ch" | "capitulo" | "kapitel" | "scene" | "part"
            | "partie" | "teil",
            rest @ ..,
        ] => rest.iter().all(|w| numbered(w)),
        [letter, "part"] if letter.len() == 1 => true,
        ["avant" | "prologue" | "episode" | "main" | "story"] => true,
        ["avant", "title" | "titre"] | ["cold", "open"] => true,
        _ => false,
    }
}

/// The kind a built-in name announces. A name is cut at its separators ("Opening - Medley",
/// "OP1: Song", "♪ Ending") and each part is compared whole, after its numbering ("01", "2",
/// "v2") is set aside, so "Opening Night" or "The Ending of Act I" do not match.
fn classify_builtin(title: &str) -> Option<MarkerType> {
    let full = clean_text(title);
    if NOT_CREDITS.iter().any(|phrase| full.contains(phrase)) {
        return None;
    }
    for part in name_parts(title) {
        let head = strip_numbering(&part);
        if head.is_empty() {
            continue;
        }
        let found = if INTRO_NAMES.contains(&head.as_str()) {
            Some(MarkerType::Intro)
        } else if CREDITS_NAMES.contains(&head.as_str()) {
            Some(MarkerType::Credits)
        } else if RECAP_NAMES.contains(&head.as_str())
            || RECAP_PREFIXES.iter().any(|p| head.starts_with(p))
            || head.ends_with(" recap")
        {
            Some(MarkerType::Recap)
        } else if PREVIEW_NAMES.contains(&head.as_str())
            || PREVIEW_PREFIXES.iter().any(|p| head.starts_with(p))
            || head.ends_with(" preview")
        {
            Some(MarkerType::Preview)
        } else {
            None
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// The name split at its separators, each part normalized like [`clean_text`].
fn name_parts(title: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for c in title.chars() {
        if matches!(
            c,
            '-' | '–'
                | '—'
                | '/'
                | ':'
                | '('
                | ')'
                | '['
                | ']'
                | '|'
                | ','
                | ';'
                | '+'
                | '«'
                | '»'
                | '「'
                | '」'
                | '『'
                | '』'
                | '~'
                | '♪'
                | '·'
                | '•'
        ) {
            parts.push(clean_text(&current));
            current.clear();
        } else {
            current.push(c);
        }
    }
    parts.push(clean_text(&current));
    parts.retain(|p| !p.is_empty());
    parts
}

/// Leading numbering ("01", "3") and trailing numbering ("2", "v2", "part 2") set aside.
fn strip_numbering(words: &str) -> String {
    let mut list: Vec<&str> = words.split(' ').filter(|w| !w.is_empty()).collect();
    while list
        .first()
        .is_some_and(|w| w.chars().all(|c| c.is_ascii_digit()))
    {
        list.remove(0);
    }
    while let Some(last) = list.last() {
        let numbered = last.chars().all(|c| c.is_ascii_digit())
            || (last.len() > 1
                && last.starts_with('v')
                && last[1..].chars().all(|c| c.is_ascii_digit()));
        if numbered && list.len() > 1 {
            list.pop();
            // "v2" is cut in two by `clean_text` ("v 2").
            if list.len() > 1 && list.last() == Some(&"v") {
                list.pop();
            }
        } else {
            break;
        }
    }
    list.join(" ")
}

/// Normalize text: lowercase, ASCII transliteration of Latin accents, punctuation dropped, a
/// space between letters and the number glued to them ("OP1" → "op 1"). Other scripts (kana,
/// kanji) are kept as they are.
pub(crate) fn clean_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut previous_alpha = false;
    for c in s.chars() {
        for lc in c.to_lowercase() {
            let mapped = match lc {
                'é' | 'è' | 'ê' | 'ë' => Some('e'),
                'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => Some('a'),
                'î' | 'ï' | 'í' | 'ì' => Some('i'),
                'ô' | 'ö' | 'ó' | 'ò' | 'õ' => Some('o'),
                'ù' | 'û' | 'ü' | 'ú' => Some('u'),
                'ç' => Some('c'),
                'ñ' => Some('n'),
                'ß' => Some('s'),
                'a'..='z' | '0'..='9' => Some(lc),
                ' ' | '-' | '_' | '\'' | '’' | '.' | '\t' => Some(' '),
                other if other.is_alphanumeric() && !other.is_ascii() => Some(other),
                _ => None,
            };
            let Some(mapped) = mapped else { continue };
            if mapped.is_ascii_digit() && previous_alpha {
                out.push(' ');
            }
            previous_alpha = mapped.is_ascii_lowercase();
            out.push(mapped);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
