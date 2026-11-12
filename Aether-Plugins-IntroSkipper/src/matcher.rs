use crate::types::{FingerprintMatch, IntroSkipperConfig};

/// Chromaprint fingerprint items per second: 11025 Hz audio, one item every 1365 samples
/// (about 123.8 ms). Measured on real `ffmpeg -f chromaprint` output: a repeated 60 s theme spans
/// 60 s x 8.0769 items, and 8.6133 items per second shifts every timestamp by 6 %.
pub const DEFAULT_FINGERPRINT_RATE: f64 = 11025.0 / 1365.0;

/// Rate announced by Aether hosts until 7 November 2026 (11025 / 1280). No Chromaprint stream
/// has it; it is a mislabel of the same data, so it is read as [`DEFAULT_FINGERPRINT_RATE`].
pub const LEGACY_FINGERPRINT_RATE: f64 = 8.61328125;

/// The rate to use for a host-supplied value: missing or non-finite means the Chromaprint rate, and
/// the legacy mislabel is corrected.
pub fn normalize_rate(rate: Option<f64>) -> f64 {
    match rate {
        Some(r) if r.is_finite() && (r - LEGACY_FINGERPRINT_RATE).abs() < 0.001 => {
            DEFAULT_FINGERPRINT_RATE
        }
        Some(r) if r.is_finite() => r,
        _ => DEFAULT_FINGERPRINT_RATE,
    }
}

/// Shortest run of similar audio that may be joined to its neighbour (intro-skipper's value).
const BRIDGE_MIN_RUN_SECONDS: f64 = 8.0;
/// Longest gap joined, as a share of the shorter of the two runs.
const BRIDGE_MAX_GAP_SHARE: f64 = 0.5;

/// Bits of each band indexing the second episode, and the number of bands in a 32-bit word.
const BAND_BITS: usize = 16;
const BANDS: usize = 32 / BAND_BITS;
const BAND_KEYS: usize = 1 << BAND_BITS;

/// Bucket of one band of a word: the band number, then the band's bits.
#[inline(always)]
fn band_key(word: u32, band: usize) -> usize {
    (band << BAND_BITS) | ((word >> (band * BAND_BITS)) as usize & (BAND_KEYS - 1))
}

/// Bit error threshold for coarse match candidate (8 bits out of 32 = 75% similarity)
const COARSE_MAX_HAMMING: u32 = 8;

/// Fast acoustic fingerprint matcher using bitwise XOR and POPCNT SIMD acceleration
pub struct FingerprintMatcher<'a> {
    config: &'a IntroSkipperConfig,
    rate: f64,
}

impl<'a> FingerprintMatcher<'a> {
    pub fn new(config: &'a IntroSkipperConfig, rate: Option<f64>) -> Self {
        Self {
            config,
            rate: normalize_rate(rate).max(1.0),
        }
    }

    /// Calculate bit agreement rate between two 32-bit frames (1.0 = identical, 0.0 = completely inverted)
    #[inline(always)]
    pub fn frame_similarity(a: u32, b: u32) -> f64 {
        let diff = (a ^ b).count_ones();
        1.0 - (diff as f64 / 32.0)
    }

    /// Check whether a fingerprint slice is silent or degenerate (less than 8 unique words).
    /// Executed in O(1) space without heap allocation.
    #[inline]
    pub fn is_silent_or_flat(slice: &[u32]) -> bool {
        if slice.is_empty() {
            return true;
        }
        let mut unique = [0u32; 8];
        let mut count = 0;
        for &word in slice.iter().take(8192) {
            if !unique[..count].contains(&word) {
                unique[count] = word;
                count += 1;
                if count >= 8 {
                    return false;
                }
            }
        }
        true
    }

    /// Match two fingerprint sequences and find common repeated audio segment
    pub fn find_best_match(&self, a: &[u32], b: &[u32]) -> Option<FingerprintMatch> {
        if a.is_empty() || b.is_empty() {
            return None;
        }
        // Limit search to configured search window (e.g. first 10 minutes)
        let max_frames_search = (self.config.search_window_end * self.rate).round() as usize;
        let start_frame_search = (self.config.search_window_start * self.rate).round() as usize;

        let a_end = a.len().min(max_frames_search);
        let b_end = b.len().min(max_frames_search);

        if start_frame_search >= a_end || start_frame_search >= b_end {
            return None;
        }

        let slice_a = &a[start_frame_search..a_end];
        let slice_b = &b[start_frame_search..b_end];
        // Inspect the searched region: unrelated audio outside the configured
        // window must not make a silent window eligible for matching.
        if Self::is_silent_or_flat(slice_a) || Self::is_silent_or_flat(slice_b) {
            return None;
        }

        let min_match_frames = (self.config.min_intro_duration * self.rate).round() as usize;
        let max_match_frames = (self.config.max_intro_duration * self.rate).round() as usize;

        if slice_a.len() < min_match_frames || slice_b.len() < min_match_frames {
            return None;
        }

        // Independent bands of BAND_BITS bits index the second episode (BANDS * 2^BAND_BITS flat
        // buckets). Candidate pairs are still checked with the full Hamming distance. Half-words
        // rather than bytes: a real fingerprint repeats some byte values so often that byte
        // buckets made every frame meet a large share of the other episode, a cost that grew with
        // the product of the two windows and exhausted the fuel once the windows reached 10 min
        // and 7 min 30. A frame of a shared passage differs by a few bits at most, so one of its
        // halves is almost always exact. The buckets are two flat arrays (offsets, then frame
        // numbers in ascending order): growing vectors cost more than the matching in the Wasm
        // sandbox.
        let mut starts = vec![0u32; BANDS * BAND_KEYS + 1];
        for &word in slice_b {
            for band in 0..BANDS {
                starts[band_key(word, band) + 1] += 1;
            }
        }
        for key in 0..BANDS * BAND_KEYS {
            starts[key + 1] += starts[key];
        }
        let mut next = starts.clone();
        let mut frames = vec![0u32; BANDS * slice_b.len()];
        for (j, &word) in slice_b.iter().enumerate() {
            for band in 0..BANDS {
                let key = band_key(word, band);
                frames[next[key] as usize] = j as u32;
                next[key] += 1;
            }
        }
        // One counter per diagonal (i - j), indexed densely: a hash map here dominated the cost
        // (hundreds of thousands of updates per pair on sustained audio, SipHash each time) and
        // made three episodes exceed the Core's 200 M fuel ceiling.
        let (n, m) = (slice_a.len(), slice_b.len());
        let mut diagonal_hits = vec![0u32; n + m - 1];
        let mut seen = vec![usize::MAX; slice_b.len()];
        for (i, &fa) in slice_a.iter().enumerate() {
            for band in 0..BANDS {
                let key = band_key(fa, band);
                for &j in &frames[starts[key] as usize..starts[key + 1] as usize] {
                    let j = j as usize;
                    if seen[j] != i && (fa ^ slice_b[j]).count_ones() <= COARSE_MAX_HAMMING {
                        seen[j] = i;
                        diagonal_hits[i + m - 1 - j] += 1;
                    }
                }
            }
        }

        // Phase 2: Filter the top candidate diagonals
        let min_hits_threshold = min_match_frames as u32 / 3;
        let mut candidates: Vec<(i64, u32)> = diagonal_hits
            .iter()
            .enumerate()
            .filter(|&(_, &hits)| hits > 0 && hits >= min_hits_threshold)
            .map(|(index, &hits)| (index as i64 - (m as i64 - 1), hits))
            .collect();

        // Most hits first; equal counts keep the lowest diagonal so the result never depends on
        // iteration order.
        candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

        let mut best_match: Option<FingerprintMatch> = None;

        // Phase 3: Fine scan along each top candidate diagonal
        for (diag, _) in candidates.iter().take(10) {
            if let Some(m) =
                self.scan_diagonal(slice_a, slice_b, *diag, min_match_frames, max_match_frames)
            {
                // Adjust offsets back by start_frame_search
                let adjusted = FingerprintMatch {
                    offset_a: (m.offset_a * self.rate + start_frame_search as f64) / self.rate,
                    offset_b: (m.offset_b * self.rate + start_frame_search as f64) / self.rate,
                    duration: m.duration,
                    score: m.score,
                };

                let replace = match &best_match {
                    None => true,
                    Some(current) => {
                        adjusted.score * adjusted.duration > current.score * current.duration
                    }
                };

                if replace {
                    best_match = Some(adjusted);
                }
            }
        }

        best_match
    }

    /// Fine scan on a specific diagonal offset
    fn scan_diagonal(
        &self,
        a: &[u32],
        b: &[u32],
        diag: i64,
        min_frames: usize,
        max_frames: usize,
    ) -> Option<FingerprintMatch> {
        let (start_i, start_j) = if diag >= 0 {
            (diag as usize, 0)
        } else {
            (0, (-diag) as usize)
        };

        let len = (a.len().saturating_sub(start_i)).min(b.len().saturating_sub(start_j));
        if len < min_frames {
            return None;
        }

        // Every run of similar audio on this diagonal, tolerating drops of ~1.5 s: (start, length,
        // summed similarity). A run is kept from 8 s on, so two parts of one sequence split by a
        // silence or a logo sting can still be joined below.
        let bridge_min = (BRIDGE_MIN_RUN_SECONDS * self.rate).round() as usize;
        let keep_from = min_frames.min(bridge_min).max(1);
        let mut runs: Vec<(usize, usize, f64)> = Vec::new();
        let mut cur_start: usize = 0;
        let mut cur_len: usize = 0;
        let mut cur_score_sum: f64 = 0.0;
        let mut consecutive_misses: usize = 0;
        let max_misses: usize = (1.5 * self.rate).round() as usize; // Allow ~1.5 sec of variation
        let threshold = self.config.similarity_threshold;
        let mut close = |start: usize, len: usize, misses: usize, sum: f64| {
            let effective_len = len.saturating_sub(misses);
            if effective_len >= keep_from && sum / (len as f64) >= threshold {
                runs.push((
                    start,
                    effective_len,
                    sum * effective_len as f64 / len as f64,
                ));
            }
        };

        for k in 0..len {
            let sim = Self::frame_similarity(a[start_i + k], b[start_j + k]);
            if sim >= threshold {
                if cur_len == 0 {
                    cur_start = k;
                }
                cur_len += 1;
                cur_score_sum += sim;
                consecutive_misses = 0;
            } else {
                consecutive_misses += 1;
                if consecutive_misses > max_misses {
                    if cur_len > 0 {
                        close(cur_start, cur_len, consecutive_misses - 1, cur_score_sum);
                    }
                    cur_len = 0;
                    cur_score_sum = 0.0;
                    consecutive_misses = 0;
                } else if cur_len > 0 {
                    cur_len += 1;
                    cur_score_sum += sim;
                }
            }
        }
        if cur_len > 0 {
            close(cur_start, cur_len, consecutive_misses, cur_score_sum);
        }

        // Like intro-skipper's run bridging: two neighbouring runs of at least 8 s join across a
        // gap of at most half the shorter one, while the whole stays within the maximum. End
        // credits interrupted by a studio logo, or an opening split by a title card, are then one
        // segment instead of their longer half.
        let mut joined = true;
        while joined {
            joined = false;
            let mut i = 0;
            while i + 1 < runs.len() {
                let (s1, l1, sum1) = runs[i];
                let (s2, l2, sum2) = runs[i + 1];
                let shorter = l1.min(l2);
                let gap = s2.saturating_sub(s1 + l1);
                let total = s2 + l2 - s1;
                if shorter >= bridge_min
                    && (gap as f64) <= shorter as f64 * BRIDGE_MAX_GAP_SHARE
                    && total <= max_frames
                {
                    // The gap's frames count as matched at the threshold, so the score stays an
                    // average over the whole joined segment.
                    runs[i] = (s1, total, sum1 + sum2 + gap as f64 * threshold);
                    runs.remove(i + 1);
                    joined = true;
                } else {
                    i += 1;
                }
            }
        }

        let mut best_segment: Option<(usize, usize, f64)> = None; // start, length, average_score
        for (start, length, sum) in runs {
            if length < min_frames || length > max_frames {
                continue;
            }
            let score = sum / length as f64;
            if best_segment.is_none_or(|(_, prev_len, prev_score)| {
                length as f64 * score > prev_len as f64 * prev_score
            }) {
                best_segment = Some((start, length, score));
            }
        }

        best_segment.map(|(seg_start, seg_len, score)| {
            let offset_a = (start_i + seg_start) as f64 / self.rate;
            let offset_b = (start_j + seg_start) as f64 / self.rate;
            let duration = seg_len as f64 / self.rate;
            FingerprintMatch {
                offset_a,
                offset_b,
                duration,
                score,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random words (xorshift), so unrelated audio never matches.
    fn noise(seed: u32, len: usize) -> Vec<u32> {
        let mut x = seed.max(1);
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x
            })
            .collect()
    }

    /// End credits whose first part and second part are separated by a logo sting that differs
    /// between episodes: both parts must come back as one segment, not the longer half.
    #[test]
    fn credits_split_by_a_sting_are_joined() {
        let rate = DEFAULT_FINGERPRINT_RATE;
        let secs = |s: f64| (s * rate).round() as usize;
        let part_one = noise(7, secs(28.0));
        let part_two = noise(11, secs(36.0));
        let build = |sting: u32, lead: u32| {
            let mut v = noise(lead, secs(20.0));
            v.extend(&part_one);
            v.extend(noise(sting, secs(4.0)));
            v.extend(&part_two);
            v
        };
        let (a, b) = (build(101, 201), build(102, 202));
        let config = IntroSkipperConfig {
            min_intro_duration: 10.0,
            max_intro_duration: 300.0,
            search_window_end: 300.0,
            ..Default::default()
        };
        let found = FingerprintMatcher::new(&config, Some(rate))
            .find_best_match(&a, &b)
            .expect("shared credits");
        assert!(
            (found.offset_a - 20.0).abs() < 1.0,
            "start {}",
            found.offset_a
        );
        assert!(
            (found.duration - 68.0).abs() < 2.0,
            "duration {}",
            found.duration
        );
    }

    /// Two shared parts far apart (a gap longer than half the shorter part) stay separate.
    #[test]
    fn distant_shared_parts_are_not_joined() {
        let rate = DEFAULT_FINGERPRINT_RATE;
        let secs = |s: f64| (s * rate).round() as usize;
        let part_one = noise(7, secs(20.0));
        let part_two = noise(11, secs(30.0));
        let build = |gap: u32| {
            let mut v = part_one.clone();
            v.extend(noise(gap, secs(40.0)));
            v.extend(&part_two);
            v
        };
        let (a, b) = (build(5), build(6));
        let config = IntroSkipperConfig {
            min_intro_duration: 10.0,
            max_intro_duration: 300.0,
            search_window_end: 300.0,
            ..Default::default()
        };
        let found = FingerprintMatcher::new(&config, Some(rate))
            .find_best_match(&a, &b)
            .expect("shared part");
        assert!(found.duration < 32.0, "duration {}", found.duration);
    }
}
