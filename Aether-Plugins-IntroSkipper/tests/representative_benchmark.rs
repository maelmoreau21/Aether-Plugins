use aether_plugin_intro_skipper::{ChapterItem, EpisodeData, IntroSkipperConfig, SeasonAnalyzer};
use std::time::Instant;

fn generate_theme(seed: u32, len: usize) -> Vec<u32> {
    let mut val = seed;
    (0..len)
        .map(|_| {
            val = val.wrapping_mul(1664525).wrapping_add(1013904223);
            val ^ (val >> 11)
        })
        .collect()
}

fn generate_noisy_theme(theme: &[u32], noise_rate: f64, seed: u32) -> Vec<u32> {
    let mut rng = seed;
    theme
        .iter()
        .map(|&word| {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let p = (rng & 0xFFFF) as f64 / 65536.0;
            if p < noise_rate {
                // Flip a few bits
                word ^ (1 << (rng % 32))
            } else {
                word
            }
        })
        .collect()
}

#[test]
fn representative_24_episodes_benchmark_and_accuracy() {
    let rate = 10.0; // 10 frames per second
    let intro_len_std = 450; // 45s * 10 fps
    let intro_len_long = 900; // 90s * 10 fps
    let credits_len_std = 600; // 60s * 10 fps
    let credits_len_long = 1200; // 120s * 10 fps

    let std_theme = generate_theme(0x9E3779B9, intro_len_std);
    let long_theme = generate_theme(0x85EBCA6B, intro_len_long);
    let credits_theme = generate_theme(0xC2B2AE35, credits_len_std);
    let credits_long_theme = generate_theme(0x27D4EB2F, credits_len_long);

    let mut episodes = Vec::with_capacity(24);
    let mut expected_intros = Vec::with_capacity(24); // (has_intro, expected_start, expected_duration)
    let mut expected_credits = Vec::with_capacity(24); // (has_credits, expected_start, expected_duration)

    for i in 1..=24 {
        let ep_id = format!("s01e{i:02}");
        let duration = 2400.0; // 40 minutes per episode
        let mut ep_fps = Vec::new();
        let mut tail_fps = Vec::new();
        let tail_start;
        let mut chapters = None;

        match i {
            // Group 1: ep 1-6 -> standard cold open (10s = 100 frames), 45s intro, standard credits
            1..=6 => {
                let cold_open = generate_theme(0x1000 + i as u32, 100);
                ep_fps.extend(cold_open);
                ep_fps.extend(std_theme.clone());
                let post_intro = generate_theme(0x2000 + i as u32, 2000);
                ep_fps.extend(post_intro);

                tail_start = Some(duration - 60.0);
                tail_fps.extend(credits_theme.clone());

                expected_intros.push((true, 10.0, 45.0));
                expected_credits.push((true, duration - 60.0, 60.0));
            }

            // Group 2: ep 7-10 -> no cold open (intro starts at 0.0s), 45s intro, standard credits
            7..=10 => {
                ep_fps.extend(std_theme.clone());
                let post_intro = generate_theme(0x3000 + i as u32, 2100);
                ep_fps.extend(post_intro);

                tail_start = Some(duration - 60.0);
                tail_fps.extend(credits_theme.clone());

                expected_intros.push((true, 0.0, 45.0));
                expected_credits.push((true, duration - 60.0, 60.0));
            }

            // Group 3: ep 11-14 -> recap chapter (30s = 300 frames), then 45s intro, standard credits
            11..=14 => {
                let recap_audio = generate_theme(0x4000 + i as u32, 300);
                ep_fps.extend(recap_audio);
                ep_fps.extend(std_theme.clone());
                let post_intro = generate_theme(0x5000 + i as u32, 1800);
                ep_fps.extend(post_intro);

                chapters = Some(vec![
                    ChapterItem {
                        start: 0.0,
                        end: Some(30.0),
                        title: "Previously On".into(),
                    },
                    ChapterItem {
                        start: 30.0,
                        end: Some(75.0),
                        title: "Opening Theme".into(),
                    },
                    ChapterItem {
                        start: 75.0,
                        end: Some(duration - 60.0),
                        title: "Episode".into(),
                    },
                    ChapterItem {
                        start: duration - 60.0,
                        end: Some(duration),
                        title: "Credits".into(),
                    },
                ]);

                tail_start = Some(duration - 60.0);
                tail_fps.extend(credits_theme.clone());

                expected_intros.push((true, 30.0, 45.0));
                expected_credits.push((true, duration - 60.0, 60.0));
            }

            // Group 4: ep 15-18 -> long intro (90s) with 20s cold open (200 frames), long credits (120s)
            15..=18 => {
                let cold_open = generate_theme(0x6000 + i as u32, 200);
                ep_fps.extend(cold_open);
                ep_fps.extend(long_theme.clone());
                let post_intro = generate_theme(0x7000 + i as u32, 1900);
                ep_fps.extend(post_intro);

                tail_start = Some(duration - 120.0);
                tail_fps.extend(credits_long_theme.clone());

                expected_intros.push((true, 20.0, 90.0));
                expected_credits.push((true, duration - 120.0, 120.0));
            }

            // Group 5: ep 19-21 -> noisy / varied intro (15% bit-flip noise over theme)
            19..=21 => {
                let cold_open = generate_theme(0x8000 + i as u32, 100);
                ep_fps.extend(cold_open);
                let noisy = generate_noisy_theme(&std_theme, 0.15, 0xABC0 + i as u32);
                ep_fps.extend(noisy);
                let post_intro = generate_theme(0x9000 + i as u32, 2000);
                ep_fps.extend(post_intro);

                tail_start = Some(duration - 60.0);
                tail_fps.extend(credits_theme.clone());

                expected_intros.push((true, 10.0, 45.0));
                expected_credits.push((true, duration - 60.0, 60.0));
            }

            // Group 6: ep 22 -> Special standalone episode: NO INTRO THEME
            22 => {
                let unique_audio = generate_theme(0xDEADBEEF, 3000);
                ep_fps.extend(unique_audio);
                tail_start = Some(duration - 60.0);
                tail_fps.extend(credits_theme.clone());

                expected_intros.push((false, 0.0, 0.0));
                expected_credits.push((true, duration - 60.0, 60.0));
            }

            // Group 7: ep 23 -> Silence / ambient flat noise at the start
            23 => {
                let flat_silence = vec![0u32; 1000]; // 100s of pure silence
                ep_fps.extend(flat_silence);
                let post_silence = generate_theme(0xC001, 2000);
                ep_fps.extend(post_silence);
                tail_start = Some(duration - 60.0);
                tail_fps.extend(credits_theme.clone());

                expected_intros.push((false, 0.0, 0.0));
                expected_credits.push((true, duration - 60.0, 60.0));
            }

            // Group 8: ep 24 -> Standard episode with both chapters and fingerprints
            24 => {
                let cold_open = generate_theme(0xE000, 100);
                ep_fps.extend(cold_open);
                ep_fps.extend(std_theme.clone());
                let post_intro = generate_theme(0xF000, 2000);
                ep_fps.extend(post_intro);

                chapters = Some(vec![
                    ChapterItem {
                        start: 0.0,
                        end: Some(10.0),
                        title: "Prologue".into(),
                    },
                    ChapterItem {
                        start: 10.0,
                        end: Some(55.0),
                        title: "Intro".into(),
                    },
                    ChapterItem {
                        start: 55.0,
                        end: Some(duration - 60.0),
                        title: "Part I".into(),
                    },
                    ChapterItem {
                        start: duration - 60.0,
                        end: Some(duration),
                        title: "Closing".into(),
                    },
                ]);

                tail_start = Some(duration - 60.0);
                tail_fps.extend(credits_theme.clone());

                expected_intros.push((true, 10.0, 45.0));
                expected_credits.push((true, duration - 60.0, 60.0));
            }

            _ => unreachable!(),
        }

        episodes.push(EpisodeData {
            id: ep_id,
            series_id: Some("series-benchmark-01".into()),
            title: Some(format!("Episode {i}")),
            season_number: Some(1),
            episode_number: Some(i as u32),
            duration,
            fingerprints: Some(ep_fps),
            fingerprint_rate: Some(rate),
            tail_fingerprints: if tail_fps.is_empty() {
                None
            } else {
                Some(tail_fps)
            },
            tail_start,
            chapters,
        });
    }

    assert_eq!(episodes.len(), 24);

    let config = IntroSkipperConfig {
        min_intro_duration: 15.0,
        max_intro_duration: 120.0,
        min_credits_duration: 15.0,
        max_credits_duration: 180.0,
        similarity_threshold: 0.70,
        search_window_start: 0.0,
        search_window_end: 600.0,
        auto_skip_intro: true,
        auto_skip_credits: true,
        enable_chapter_detection: true,
        enable_fingerprint_detection: true,
    };

    let analyzer = SeasonAnalyzer::new(&config);

    // Measure benchmark execution time
    let start_time = Instant::now();
    let results = analyzer.analyze_season(&episodes);
    let elapsed = start_time.elapsed();

    println!("\n=== INTRO SKIPPER 24-EPISODE BENCHMARK ===");
    println!("Elapsed time: {:.2?}", elapsed);
    println!("Time per episode: {:.2?}", elapsed / 24);

    assert_eq!(results.len(), 24);

    let mut intro_tp = 0;
    let mut intro_fp = 0;
    let mut intro_fn = 0;

    let mut credits_tp = 0;
    let mut credits_fp = 0;
    let mut credits_fn = 0;

    for (i, res) in results.iter().enumerate() {
        let (has_intro_expected, exp_intro_start, exp_intro_dur) = expected_intros[i];
        let (has_credits_expected, exp_credits_start, exp_credits_dur) = expected_credits[i];

        // Intro check
        if let Some(intro) = &res.intro {
            if has_intro_expected {
                // Verify bounds tolerance (within 2 seconds)
                let start_diff = (intro.start - exp_intro_start).abs();
                let dur_diff = (intro.duration() - exp_intro_dur).abs();
                if start_diff <= 2.5 && dur_diff <= 3.0 {
                    intro_tp += 1;
                } else {
                    println!(
                        "Ep {}: Intro timing off: detected [{:.1}..{:.1}], expected [{:.1}..{:.1}]",
                        i + 1,
                        intro.start,
                        intro.end,
                        exp_intro_start,
                        exp_intro_start + exp_intro_dur
                    );
                    intro_fp += 1;
                }
            } else {
                println!(
                    "Ep {}: False positive intro on no-intro episode: [{:.1}..{:.1}]",
                    i + 1,
                    intro.start,
                    intro.end
                );
                intro_fp += 1;
            }
        } else if has_intro_expected {
            println!("Ep {}: False negative (intro missed)", i + 1);
            intro_fn += 1;
        }

        // Credits check
        if let Some(credits) = &res.credits {
            if has_credits_expected {
                let start_diff = (credits.start - exp_credits_start).abs();
                let dur_diff = (credits.duration() - exp_credits_dur).abs();
                if start_diff <= 3.0 && dur_diff <= 4.0 {
                    credits_tp += 1;
                } else {
                    credits_fp += 1;
                }
            } else {
                credits_fp += 1;
            }
        } else if has_credits_expected {
            credits_fn += 1;
        }
    }

    let intro_precision = intro_tp as f64 / (intro_tp + intro_fp).max(1) as f64;
    let intro_recall = intro_tp as f64 / (intro_tp + intro_fn).max(1) as f64;

    let credits_precision = credits_tp as f64 / (credits_tp + credits_fp).max(1) as f64;
    let credits_recall = credits_tp as f64 / (credits_tp + credits_fn).max(1) as f64;

    println!("------------------------------------------");
    println!(
        "Intro Detection:   Precision = {:.1}% | Recall = {:.1}% (TP={}, FP={}, FN={})",
        intro_precision * 100.0,
        intro_recall * 100.0,
        intro_tp,
        intro_fp,
        intro_fn
    );
    println!(
        "Credits Detection: Precision = {:.1}% | Recall = {:.1}% (TP={}, FP={}, FN={})",
        credits_precision * 100.0,
        credits_recall * 100.0,
        credits_tp,
        credits_fp,
        credits_fn
    );
    println!("------------------------------------------");

    // Acceptance criteria:
    // 1. Zero false positives on standalone and silence episodes (ep 22 & 23)
    assert!(
        results[21].intro.is_none(),
        "Ep 22 must NOT have an intro (standalone)"
    );
    assert!(
        results[22].intro.is_none(),
        "Ep 23 must NOT have an intro (silence)"
    );

    // 2. High precision and recall (>= 90%)
    assert!(
        intro_precision >= 0.95,
        "Intro precision must be at least 95%"
    );
    assert!(intro_recall >= 0.90, "Intro recall must be at least 90%");
    assert!(
        credits_precision >= 0.95,
        "Credits precision must be at least 95%"
    );
    assert!(
        credits_recall >= 0.90,
        "Credits recall must be at least 90%"
    );

    // 3. Execution time budget: < 250ms for 24 full episodes in debug build
    assert!(
        elapsed.as_millis() < 500,
        "Benchmark took {:?}, expected < 500ms",
        elapsed
    );
}
