//! Boundaries of acoustic segments, adjusted the way a viewer expects: an opening found 1 s
//! after the start begins at 0, credits that run to the end of the file end with it, and a
//! boundary a few seconds from a chapter mark lands on the mark (chapters are authored cuts).
use crate::types::MarkerType;

/// An opening found this close to the start of the file starts at 0.
pub const START_SNAP: f64 = 2.5;
/// Credits found this close to the end of the file end with it. Chromaprint's last item comes
/// about 3 s before the audio ends, so a match can never reach the very end.
pub const END_SNAP: f64 = 6.0;
/// When the acoustic match stops at the edge of the analysed audio, the real boundary may lie
/// beyond it: a chapter mark up to this far later is taken instead.
pub const TRUNCATED_EXTENSION: f64 = 30.0;

/// The point of `points` nearest to `time` within `window` seconds, else `time`.
pub fn nearest(points: &[f64], time: f64, window: f64) -> f64 {
    if window <= 0.0 {
        return time;
    }
    points
        .iter()
        .copied()
        .filter(|p| (p - time).abs() <= window)
        .min_by(|a, b| (a - time).abs().total_cmp(&(b - time).abs()))
        .unwrap_or(time)
}

/// Adjust one acoustic segment. `analysed_end` is where the analysed audio of that part of the
/// file stops (end of the head for an opening), so a match cut there can reach a later chapter.
pub fn snap_segment(
    kind: MarkerType,
    start: f64,
    end: f64,
    duration: f64,
    boundaries: &[f64],
    window: f64,
    analysed_end: Option<f64>,
) -> (f64, f64) {
    snap_segment_in(
        kind,
        start,
        end,
        duration,
        boundaries,
        window,
        (None, analysed_end),
    )
}

/// [`snap_segment`] with both edges of the analysed audio: `(start, end)`. A match that starts
/// at the start edge (credits that begin before the analysed tail) can reach a chapter mark up
/// to [`TRUNCATED_EXTENSION`] earlier, one that stops at the end edge a mark that much later.
/// Pass the match's own boundary as an edge when the match was cut at the edge of the other
/// episode of the comparison.
pub fn snap_segment_in(
    kind: MarkerType,
    start: f64,
    end: f64,
    duration: f64,
    boundaries: &[f64],
    window: f64,
    analysed: (Option<f64>, Option<f64>),
) -> (f64, f64) {
    let mut s = start;
    let mut e = end;
    let cut_start = analysed.0.is_some_and(|edge| s <= edge + 2.0);
    if kind == MarkerType::Intro && s <= START_SNAP {
        s = 0.0;
    } else if cut_start && window > 0.0 {
        s = boundaries
            .iter()
            .copied()
            .filter(|p| *p >= s - TRUNCATED_EXTENSION && *p <= s + window)
            .min_by(|a, b| (a - s).abs().total_cmp(&(b - s).abs()))
            .unwrap_or(s);
    } else {
        s = nearest(boundaries, s, window);
    }
    let truncated = analysed.1.is_some_and(|edge| e >= edge - 2.0);
    if kind == MarkerType::Credits && duration > 0.0 && e >= duration - END_SNAP {
        e = duration;
    } else if truncated {
        e = boundaries
            .iter()
            .copied()
            .filter(|p| *p >= e - window && *p <= e + TRUNCATED_EXTENSION)
            .min_by(|a, b| (a - e).abs().total_cmp(&(b - e).abs()))
            .unwrap_or(e);
    } else {
        e = nearest(boundaries, e, window);
    }
    if duration > 0.0 {
        e = e.min(duration);
    }
    if e - s < 1.0 {
        return (start, end.min(if duration > 0.0 { duration } else { end }));
    }
    (s, e)
}

/// Share of the shorter of two segments covered by both (0 when they do not overlap).
pub fn overlap_ratio(a: (f64, f64), b: (f64, f64)) -> f64 {
    let shared = a.1.min(b.1) - a.0.max(b.0);
    let shorter = (a.1 - a.0).min(b.1 - b.0);
    if shared <= 0.0 || shorter <= 0.0 {
        0.0
    } else {
        shared / shorter
    }
}
