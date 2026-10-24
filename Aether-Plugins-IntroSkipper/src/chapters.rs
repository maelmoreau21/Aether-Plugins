use crate::types::{ChapterItem, DetectionMethod, IntroMarker, IntroSkipperConfig, MarkerType};

/// Fast normalized title classification for chapter metadata
pub struct ChapterClassifier<'a> {
    config: &'a IntroSkipperConfig,
}

impl<'a> ChapterClassifier<'a> {
    pub fn new(config: &'a IntroSkipperConfig) -> Self {
        Self { config }
    }

    /// Check if chapter title matches common intro patterns
    pub fn is_intro_title(title: &str) -> bool {
        let clean = clean_text(title);
        matches!(
            clean.as_str(),
            "intro"
                | "introduction"
                | "opening"
                | "opening credits"
                | "opening theme"
                | "op"
                | "generique"
                | "generique debut"
                | "generique d ouverture"
                | "ouverture"
                | "title"
                | "main title"
                | "theme song"
        ) || clean.starts_with("intro ")
            || clean.starts_with("opening ")
            || clean.starts_with("op ")
            || clean.starts_with("generique debut")
    }

    /// Check if chapter title matches common credits/outro patterns
    pub fn is_credits_title(title: &str) -> bool {
        let clean = clean_text(title);
        matches!(
            clean.as_str(),
            "credits"
                | "credit"
                | "end credits"
                | "ending credits"
                | "ending"
                | "ending theme"
                | "ed"
                | "outro"
                | "closing"
                | "closing credits"
                | "generique de fin"
                | "generique fin"
                | "fin"
        ) || clean.starts_with("credits ")
            || clean.starts_with("ending ")
            || clean.starts_with("ed ")
            || clean.starts_with("generique de fin")
    }

    /// Check if chapter title matches recap patterns
    pub fn is_recap_title(title: &str) -> bool {
        let clean = clean_text(title);
        matches!(
            clean.as_str(),
            "recap" | "previously" | "previously on" | "precedemment" | "resume"
        )
    }

    /// Parse markers from an episode's chapter list
    pub fn parse_markers(
        &self,
        episode_id: &str,
        chapters: &[ChapterItem],
        episode_duration: f64,
    ) -> Vec<IntroMarker> {
        let mut markers = Vec::new();
        if chapters.is_empty() {
            return markers;
        }

        for (i, chapter) in chapters.iter().enumerate() {
            let start = chapter.start.max(0.0);
            let next_start = if let Some(end) = chapter.end {
                end
            } else if i + 1 < chapters.len() {
                chapters[i + 1].start
            } else {
                episode_duration
            };

            let end = next_start.min(episode_duration);
            let duration = (end - start).max(0.0);

            if Self::is_intro_title(&chapter.title) {
                if duration >= self.config.min_intro_duration
                    && duration <= self.config.max_intro_duration
                {
                    markers.push(IntroMarker {
                        id: format!("{episode_id}-intro-chap-{i}"),
                        start,
                        end,
                        marker_type: MarkerType::Intro,
                        confidence: 0.98,
                        method: DetectionMethod::Chapter,
                    });
                }
            } else if Self::is_credits_title(&chapter.title) {
                if duration >= self.config.min_credits_duration
                    && duration <= self.config.max_credits_duration
                {
                    markers.push(IntroMarker {
                        id: format!("{episode_id}-credits-chap-{i}"),
                        start,
                        end,
                        marker_type: MarkerType::Credits,
                        confidence: 0.98,
                        method: DetectionMethod::Chapter,
                    });
                }
            } else if Self::is_recap_title(&chapter.title) && (5.0..=180.0).contains(&duration) {
                markers.push(IntroMarker {
                    id: format!("{episode_id}-recap-chap-{i}"),
                    start,
                    end,
                    marker_type: MarkerType::Recap,
                    confidence: 0.95,
                    method: DetectionMethod::Chapter,
                });
            }
        }

        markers
    }
}

/// Normalize text: lowercase, ASCII transliteration of French accents, strip punctuation
fn clean_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let lower = c.to_lowercase();
        for lc in lower {
            match lc {
                'é' | 'è' | 'ê' | 'ë' => out.push('e'),
                'à' | 'â' | 'ä' => out.push('a'),
                'î' | 'ï' => out.push('i'),
                'ô' | 'ö' => out.push('o'),
                'ù' | 'û' | 'ü' => out.push('u'),
                'ç' => out.push('c'),
                'a'..='z' | '0'..='9' => out.push(lc),
                ' ' | '-' | '_' | '\'' => out.push(' '),
                _ => {}
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
