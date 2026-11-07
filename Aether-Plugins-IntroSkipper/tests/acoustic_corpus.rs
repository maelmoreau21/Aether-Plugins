//! Real Chromaprint fingerprints of synthesised episodes (see `tests/corpus/build-corpus.mjs`).
//!
//! Every scenario is scored against a ground truth with a tolerance of `TOLERANCE` seconds on both
//! boundaries. Run `cargo test --release -p aether-plugin-intro-skipper --test acoustic_corpus --
//! --nocapture` to print the table; the numbers are measurements on synthetic audio, not a claim
//! about real series.
use aether_plugin_intro_skipper::{
    EpisodeData, EpisodeDetectionResult, IntroSkipperConfig, SeasonAnalyzer,
};
use serde_json::Value;
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// Counts live and peak heap bytes so the memory of one analysis can be reported.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        if !new.is_null() {
            if new_size >= layout.size() {
                let live = LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed) + new_size
                    - layout.size();
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;
/// Tests that measure memory must not overlap (a failed test must not poison the others).
static SERIAL: Mutex<()> = Mutex::new(());

/// A Chromaprint item summarises about 1.5 s of audio before its timestamp, so a start is read up to
/// ~2.4 s early on this corpus and a credits end ~2.6 s early; this is the tolerance, not a promise about real series.
const TOLERANCE: f64 = 3.0;
const SERIES: &str = "series-corpus";

fn corpus() -> (PathBuf, Value) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/intro");
    let manifest =
        serde_json::from_slice(&std::fs::read(dir.join("manifest.json")).expect("manifest.json"))
            .unwrap();
    (dir, manifest)
}

fn words(dir: &std::path::Path, file: &str) -> Vec<u32> {
    let bytes = std::fs::read(dir.join(file)).unwrap();
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect()
}

fn episodes(
    dir: &std::path::Path,
    manifest: &Value,
    list: &[Value],
    rate: f64,
) -> Vec<EpisodeData> {
    let duration = manifest["duration"].as_f64().unwrap();
    let tail_seconds = manifest["tailSeconds"].as_f64().unwrap();
    list.iter()
        .map(|ep| EpisodeData {
            id: ep["id"].as_str().unwrap().into(),
            series_id: Some(SERIES.into()),
            title: None,
            season_number: Some(1),
            episode_number: ep["number"].as_u64().map(|n| n as u32),
            duration,
            fingerprints: Some(words(dir, ep["head"].as_str().unwrap())),
            fingerprint_rate: Some(rate),
            tail_fingerprints: ep["tail"].as_str().map(|f| words(dir, f)),
            // The Node asks for the last `tailSeconds` seconds.
            tail_start: ep["tail"].as_str().map(|_| duration - tail_seconds),
            chapters: None,
        })
        .collect()
}

fn range(value: &Value) -> Option<(f64, f64)> {
    value
        .as_object()
        .map(|o| (o["start"].as_f64().unwrap(), o["end"].as_f64().unwrap()))
}

#[derive(Default, Debug, Clone, Copy)]
struct Score {
    located: u32,
    misplaced: u32,
    missed: u32,
    spurious: u32,
    absent_ok: u32,
}

impl Score {
    fn add(&mut self, truth: Option<(f64, f64)>, found: Option<(f64, f64)>) -> Option<(f64, f64)> {
        match (truth, found) {
            (Some((ts, te)), Some((fs, fe))) => {
                let (ds, de) = ((fs - ts).abs(), (fe - te).abs());
                if ds <= TOLERANCE && de <= TOLERANCE {
                    self.located += 1;
                } else {
                    self.misplaced += 1;
                }
                Some((fs - ts, fe - te))
            }
            (Some(_), None) => {
                self.missed += 1;
                None
            }
            (None, Some(_)) => {
                self.spurious += 1;
                None
            }
            (None, None) => {
                self.absent_ok += 1;
                None
            }
        }
    }
}

struct Run {
    name: String,
    intro: Score,
    credits: Score,
    results: Vec<EpisodeDetectionResult>,
    intro_errors: Vec<Option<(f64, f64)>>,
    credits_errors: Vec<Option<(f64, f64)>>,
    micros: u128,
    peak_bytes: usize,
}

fn run(
    config: &IntroSkipperConfig,
    dir: &std::path::Path,
    manifest: &Value,
    name: &str,
    list: &[Value],
    rate: f64,
) -> Run {
    let eps = episodes(dir, manifest, list, rate);
    let analyzer = SeasonAnalyzer::new(config);
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let started = Instant::now();
    let results = analyzer.analyze_season(&eps);
    let micros = started.elapsed().as_micros();
    let peak_bytes = PEAK.load(Ordering::Relaxed).saturating_sub(baseline);
    let mut run = Run {
        name: name.into(),
        intro: Score::default(),
        credits: Score::default(),
        intro_errors: Vec::new(),
        credits_errors: Vec::new(),
        results: Vec::new(),
        micros,
        peak_bytes,
    };
    for (ep, result) in list.iter().zip(&results) {
        assert_eq!(result.episode_id, ep["id"].as_str().unwrap());
        let found_intro = result.intro.as_ref().map(|m| (m.start, m.end));
        let e = run.intro.add(range(&ep["intro"]), found_intro);
        run.intro_errors.push(e);
        // Credits are scored only where the scenario describes them.
        if ep.get("credits").is_some() {
            let found = result.credits.as_ref().map(|m| (m.start, m.end));
            let e = run.credits.add(range(&ep["credits"]), found);
            run.credits_errors.push(e);
        }
    }
    run.results = results;
    run
}

fn config() -> IntroSkipperConfig {
    // Defaults of the plugin: what an installation without custom settings runs.
    IntroSkipperConfig::default()
}

fn find<'a>(runs: &'a [Run], name: &str) -> &'a Run {
    runs.iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("scenario {name}"))
}

fn print_table(runs: &[Run]) {
    println!(
        "\n{:<18} {:>3} | {:^28} | {:^28} | {:>8} {:>9}",
        "scenario",
        "eps",
        "intro ok/off/miss/spur/none",
        "credits ok/off/miss/spur",
        "ms",
        "peak KiB"
    );
    for r in runs {
        let i = r.intro;
        let c = r.credits;
        println!(
            "{:<18} {:>3} | {:>5}/{:>3}/{:>4}/{:>4}/{:>4}   | {:>6}/{:>3}/{:>4}/{:>4}        | {:>8.1} {:>9}",
            r.name,
            r.results.len(),
            i.located,
            i.misplaced,
            i.missed,
            i.spurious,
            i.absent_ok,
            c.located,
            c.misplaced,
            c.missed,
            c.spurious,
            r.micros as f64 / 1000.0,
            r.peak_bytes / 1024
        );
    }
}

fn median_abs(errors: &[Option<(f64, f64)>], end: bool) -> String {
    let mut v: Vec<f64> = errors
        .iter()
        .flatten()
        .map(|e| if end { e.1 } else { e.0 }.abs())
        .collect();
    if v.is_empty() {
        return "-".into();
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    format!("{:.1}", v[v.len() / 2])
}

fn all_runs(rate: f64) -> Vec<Run> {
    let (dir, manifest) = corpus();
    let config = config();
    manifest["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            run(
                &config,
                &dir,
                &manifest,
                s["name"].as_str().unwrap(),
                s["episodes"].as_array().unwrap(),
                rate,
            )
        })
        .collect()
}

fn print_errors(runs: &[Run]) {
    println!(
        "{:<18} median |error| s: intro start/end | credits start/end",
        "scenario"
    );
    for r in runs {
        println!(
            "{:<18} {:>8}/{:<8} | {:>8}/{:<8}",
            r.name,
            median_abs(&r.intro_errors, false),
            median_abs(&r.intro_errors, true),
            median_abs(&r.credits_errors, false),
            median_abs(&r.credits_errors, true)
        );
    }
}

#[test]
fn acoustic_corpus_accuracy_cost_and_known_limits() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (_, manifest) = corpus();
    let real = manifest["sampleRate"].as_f64().unwrap();
    let runs = all_runs(real);
    print_table(&runs);
    print_errors(&runs);

    // Scenarios where every episode must be found, once, at the right place.
    for name in [
        "standard",
        "cold_open",
        "recap",
        "opening_change",
        "special_episode",
        "noisy",
        "other_encode",
        "truncated_intro",
        "credits_variants",
    ] {
        let r = find(&runs, name);
        assert_eq!(
            (r.intro.misplaced, r.intro.missed, r.intro.spurious),
            (0, 0, 0),
            "{name}: {:?} {:?}",
            r.intro,
            r.intro_errors
        );
    }
    // The special episode (no opening) and the anthology are left alone.
    assert_eq!(find(&runs, "special_episode").intro.absent_ok, 1);
    let none = find(&runs, "no_intro");
    assert_eq!((none.intro.spurious, none.intro.absent_ok), (0, 5));
    // Credits: found where they exist, absent where they do not, at the right place.
    for name in ["standard", "opening_change", "no_intro", "credits_variants"] {
        let r = find(&runs, name);
        assert_eq!(
            (r.credits.misplaced, r.credits.missed, r.credits.spurious),
            (0, 0, 0),
            "{name} credits: {:?} {:?}",
            r.credits,
            r.credits_errors
        );
    }
    assert_eq!(find(&runs, "credits_variants").credits.absent_ok, 1);
    // KNOWN LIMIT, pinned so a change is noticed: a studio bumper shared by every episode IS a
    // repetition, so it is reported as an opening. Nothing in the audio tells it apart.
    assert_eq!(find(&runs, "studio_bumper").intro.spurious, 5);
    // Cost on this machine class: far below the 35 ms/episode the documentation announces.
    for r in &runs {
        assert!(r.micros < 2_000_000, "{}: {} us", r.name, r.micros);
        assert!(r.peak_bytes < 8 << 20, "{}: {} bytes", r.name, r.peak_bytes);
    }
}

/// The Node announced 11025/1280 items per second until 7 November 2026. Old hosts must get the
/// same markers as new ones.
#[test]
fn legacy_node_rate_gives_the_same_markers() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (_, manifest) = corpus();
    let legacy = all_runs(manifest["legacyRate"].as_f64().unwrap());
    let real = all_runs(manifest["sampleRate"].as_f64().unwrap());
    for (a, b) in legacy.iter().zip(&real) {
        assert_eq!(a.results, b.results, "{}", a.name);
    }
}

/// What a host that can only send two or three episodes (see `payload_budget`) gets: every window
/// of consecutive episodes of every scenario, scored on its own. Windows that straddle a change of
/// opening or a special episode legitimately find less (a pair cannot confirm what only one has).
#[test]
fn small_batches_stay_safe() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, manifest) = corpus();
    let config = config();
    let rate = manifest["sampleRate"].as_f64().unwrap();
    for size in [2usize, 3, 5] {
        let (mut intro, mut credits) = (Score::default(), Score::default());
        let mut spurious_without_bumper = 0;
        for scenario in manifest["scenarios"].as_array().unwrap() {
            let list = scenario["episodes"].as_array().unwrap();
            for window in list.windows(size) {
                let r = run(&config, &dir, &manifest, "window", window, rate);
                if scenario["name"] != "studio_bumper" {
                    spurious_without_bumper += r.intro.spurious + r.credits.spurious;
                }
                for (a, b) in [(&mut intro, r.intro), (&mut credits, r.credits)] {
                    a.located += b.located;
                    a.misplaced += b.misplaced;
                    a.missed += b.missed;
                    a.spurious += b.spurious;
                    a.absent_ok += b.absent_ok;
                }
            }
        }
        println!("batches of {size}: intro {intro:?} credits {credits:?}");
        assert_eq!(spurious_without_bumper, 0, "size {size}");
        let truth = intro.located + intro.misplaced + intro.missed;
        assert!(intro.located * 100 >= truth * 80, "size {size}: {intro:?}");
    }
}

/// The Core serialises `{"episodes":[...]}` and refuses a guest input over `MAX_JSON` bytes.
/// This builds the same fields from real fingerprints and shows how many episodes fit.
#[test]
fn payload_budget() {
    use base64::Engine;
    let (dir, manifest) = corpus();
    let list = manifest["scenarios"][0]["episodes"].as_array().unwrap();
    let one = |i: usize| {
        let ep = &list[i % list.len()];
        let enc = |f: &str| {
            base64::engine::general_purpose::STANDARD.encode(std::fs::read(dir.join(f)).unwrap())
        };
        serde_json::json!({
            "id": format!("0b6f3c2e-6d1f-4c43-9a1e-{:012}", i),
            "seriesId": "5c1d8c52-8c3b-4d0a-8f5d-7b8d3d2f9a11",
            "seasonNumber": 1,
            "episodeNumber": i + 1,
            "duration": 1440.0,
            "fingerprintsCompact": enc(ep["head"].as_str().unwrap()),
            "fingerprintRate": 8.61328125,
            "tailFingerprintsCompact": enc(ep["tail"].as_str().unwrap()),
            "tailStart": 1254.0,
        })
    };
    let size = |n: usize| {
        let episodes: Vec<_> = (0..n).map(one).collect();
        serde_json::to_vec(&serde_json::json!({"episodes": episodes, "config": {}}))
            .unwrap()
            .len()
    };
    let limit = media_extension_sdk::MAX_JSON;
    let fits = (2..=8)
        .take_while(|n| size(*n) <= limit)
        .last()
        .unwrap_or(1);
    println!(
        "episode JSON: {} bytes each; limit {limit}; 2={} 3={} 4={} 8={}; fits {fits}",
        size(1),
        size(2),
        size(3),
        size(4),
        size(8)
    );
    // The Core builds up to eight episodes: more than the guest can receive.
    assert!(size(8) > limit);
    assert!(size(fits + 1) > limit);
    assert!(fits <= 3, "{fits}");
}
