//! Browser director integration: deterministic replay, seed variation,
//! neutral opening, selected-layer output, recovery, resets and invalid-input
//! twins, checked against an independent progression/director/history
//! pipeline built from public `spook_core` types.
use spook_core::HauntState;
use spook_web::{MirrorEngine, MODE_MIRROR};

const W: u32 = 16;
const H: u32 = 12;

/// Three alternating phases, 300 ms period: every 100 ms sample differs from
/// the previous one (high motion) and the 200 ms trail offset lands on a
/// different phase, so delayed reflection and ghost trails are both visible.
fn frame(time: u64) -> Vec<u8> {
    let phase = (time / 100) % 3;
    (0..u64::from(W * H))
        .flat_map(|i| {
            let v = ((i * 7 + phase * 85) % 256) as u8;
            [v, v / 2, 255 - v, 255]
        })
        .collect()
}

/// A constant frame: no motion.
fn still() -> Vec<u8> {
    frame(0)
}

fn engine(seed: Option<u32>) -> MirrorEngine {
    let mut engine = MirrorEngine::new(W, H).unwrap();
    engine.configure_automatic(true, false).unwrap();
    if let Some(seed) = seed {
        engine.configure_seed(seed).unwrap();
    }
    engine
}

fn reference(seed: Option<u32>) -> Reference {
    Reference::new(W as usize, H as usize, seed, false)
}

fn clear(input: &[u8]) -> Vec<u8> {
    MirrorEngine::new(W, H)
        .unwrap()
        .process_rgba(input, MODE_MIRROR, 0)
        .unwrap()
}

/// Replays `engine` and the reference over `times`, asserting equal output.
/// Returns the per-frame `(layers, state, delay, trail)`.
fn run(
    engine: &mut MirrorEngine,
    reference: &mut Reference,
    times: impl Iterator<Item = u64>,
    input: impl Fn(u64) -> Vec<u8>,
) -> Vec<(u64, u8, HauntState, u16, u8)> {
    times
        .map(|time| {
            let input = input(time);
            let out = engine.process_automatic_rgba(&input, time as f64).unwrap();
            let base = reference.step(&input, time, false);
            assert_eq!(out, base, "t={time}");
            assert_eq!(engine.haunt_strength(), reference.level.unwrap().strength);
            let snap = reference.snapshot;
            (
                time,
                snap.layers,
                reference.level.unwrap().state,
                snap.delay_ms,
                snap.trail_strength,
            )
        })
        .collect()
}

fn outputs(engine: &mut MirrorEngine, until: u64) -> Vec<Vec<u8>> {
    (0..=until)
        .step_by(100)
        .map(|t| engine.process_automatic_rgba(&frame(t), t as f64).unwrap())
        .collect()
}

/// Layers of each event: one entry per maximal run of selected frames.
fn masks(log: &[(u64, u8, HauntState, u16, u8)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut last = 0;
    for entry in log {
        if entry.1 != 0 && last == 0 {
            out.push(entry.1);
        }
        last = entry.1;
    }
    out
}

/// Independent reference for the automatic pipeline. It uses only the public
/// `spook_core` types (progression, director, history, motion) and a plain
/// `MirrorEngine::process_rgba` for the palette, never the engine's automatic
/// path, so it reproduces the pre-overlay base frame a skull is drawn over.
mod reference {
    use spook_core::{
        grayscale_rgb24, motion_gray8, DirectorConfig, DirectorInput, DirectorSnapshot,
        HauntDirector, HauntLevel, HauntProgression, RgbFrame, TemporalHistory, TemporalOptions,
    };
    use spook_web::{MirrorEngine, MODE_MIRROR, MODE_SPECTRAL};

    pub struct Reference {
        w: usize,
        h: usize,
        reduced: bool,
        progression: HauntProgression,
        director: HauntDirector,
        history: TemporalHistory,
        work: Vec<u8>,
        previous: Vec<u8>,
        sampled: Option<u64>,
        activity: u16,
        palette: MirrorEngine,
        pub level: Option<HauntLevel>,
        pub snapshot: DirectorSnapshot,
    }

    impl Reference {
        pub fn new(w: usize, h: usize, seed: Option<u32>, reduced: bool) -> Self {
            let (progression, director) = Self::session(seed);
            Self {
                w,
                h,
                reduced,
                progression,
                director,
                history: TemporalHistory::new(w, h).unwrap(),
                work: vec![0; w * h * 4],
                previous: vec![0; w * h],
                sampled: None,
                activity: 0,
                palette: MirrorEngine::new(w as u32, h as u32).unwrap(),
                level: None,
                snapshot: DirectorSnapshot::default(),
            }
        }

        fn session(seed: Option<u32>) -> (HauntProgression, HauntDirector) {
            let config = DirectorConfig::default();
            match seed {
                Some(seed) => (
                    HauntProgression::with_seed(seed).unwrap(),
                    HauntDirector::new(seed, config).unwrap(),
                ),
                // No configure_seed call: fixed timing and director seed 0.
                None => (
                    HauntProgression::new().unwrap(),
                    HauntDirector::new(0, config).unwrap(),
                ),
            }
        }

        /// One frame; returns the final mirrored RGBA before any skull overlay.
        pub fn step(&mut self, input: &[u8], now: u64, head: bool) -> Vec<u8> {
            let mut rgb = Vec::with_capacity(self.w * self.h * 3);
            for pixel in input.as_chunks::<4>().0 {
                rgb.extend_from_slice(&pixel[..3]);
            }
            let frame = RgbFrame::new(self.w, self.h, self.w * 3, &rgb).unwrap();
            let mut gray = vec![0; self.w * self.h];
            grayscale_rgb24(&frame, &mut gray).unwrap();
            let due = self
                .sampled
                .is_none_or(|last| now.saturating_sub(last) >= 100);
            if due && self.sampled.is_some() {
                self.activity = motion_gray8(&gray, &self.previous, self.w, self.h, self.w, 12)
                    .unwrap()
                    .activity;
            } else if due {
                self.activity = 0;
            }
            let level = self
                .progression
                .update(now, self.activity, self.reduced)
                .unwrap();
            if due {
                self.previous.copy_from_slice(&gray);
                self.sampled = Some(now);
            }
            self.snapshot = self
                .director
                .update(
                    now,
                    DirectorInput {
                        level,
                        head_available: head,
                        reduced_motion: self.reduced,
                        quality_budget: 2,
                    },
                )
                .unwrap();
            self.level = Some(level);
            let options = TemporalOptions {
                delay_ms: self.snapshot.delay_ms,
                trail_strength: self.snapshot.trail_strength,
            };
            self.history
                .process(input, &mut self.work, now, options)
                .unwrap();
            let intensity = (140 * u32::from(level.strength) / 6000) as u8;
            let mode = if intensity == 0 {
                MODE_MIRROR
            } else {
                MODE_SPECTRAL
            };
            self.palette
                .process_rgba(&self.work, mode, intensity)
                .unwrap()
        }
    }
}
use reference::Reference;

#[test]
fn engine_output_equals_the_reference_for_every_seed_and_only_selected_layers_act() {
    let mut union = 0u8;
    for seed in [None, Some(0), Some(1), Some(2), Some(3), Some(4)] {
        let (mut e, mut r) = (engine(seed), reference(seed));
        let log = run(&mut e, &mut r, (0..=60_000).step_by(100), frame);
        for &(time, layers, _, delay, trail) in &log {
            union |= layers;
            // An unselected layer contributes nothing.
            assert!(layers & 1 != 0 || delay == 0, "{time}");
            assert!(layers & 2 != 0 || trail == 0, "{time}");
            assert_eq!(layers & 4, 0, "no head, no SKULL");
        }
        assert!(log.iter().any(|e| e.3 > 0), "LAG selected for {seed:?}");
        assert!(log.iter().any(|e| e.4 > 0), "ECHO selected for {seed:?}");
    }
    assert_eq!(union, 3);
}

#[test]
fn the_opening_is_clear_and_the_director_only_starts_after_it() {
    for seed in [None, Some(0), Some(7)] {
        let (mut e, mut r) = (engine(seed), reference(seed));
        let log = run(&mut e, &mut r, (0..=40_000).step_by(100), frame);
        let first = log.iter().position(|entry| entry.1 != 0).unwrap();
        // While progression strength is zero the engine output is the plain
        // mirror, and no layer is selected before the opening has ended.
        let mut e = engine(seed);
        for &(time, layers, _, _, _) in &log {
            let out = e.process_automatic_rgba(&frame(time), time as f64).unwrap();
            if e.haunt_strength() == 0 {
                assert_eq!(out, clear(&frame(time)), "{seed:?} t={time}");
                assert_eq!(layers, 0);
            }
            if time >= log[first].0 {
                break;
            }
        }
        assert!(log[first].0 >= 4000 + 1500 / 2, "{seed:?}");
    }
}

#[test]
fn identical_replays_match_and_seeds_change_the_selected_layers() {
    let first = outputs(&mut engine(Some(5)), 60_000);
    assert_eq!(first, outputs(&mut engine(Some(5)), 60_000));
    let mut sequences = Vec::new();
    for seed in 0..10 {
        let (mut e, mut r) = (engine(Some(seed)), reference(Some(seed)));
        sequences.push(masks(&run(
            &mut e,
            &mut r,
            (0..=60_000).step_by(100),
            frame,
        )));
    }
    sequences.sort();
    sequences.dedup();
    // Different selections, not just different timing.
    assert!(sequences.len() >= 6, "{sequences:?}");
    let firsts: std::collections::HashSet<_> = sequences.iter().map(|s| s[0]).collect();
    assert!(firsts.len() >= 2, "{sequences:?}");
}

#[test]
fn without_configure_seed_the_session_uses_fixed_timing_and_director_seed_zero() {
    let (mut e, mut r) = (engine(None), reference(None));
    let log = run(&mut e, &mut r, (0..=60_000).step_by(100), frame);
    assert_eq!(masks(&log)[..3], [3, 2, 1]);
    // The first event is selected at 6500 ms (strength 1500 of the fixed ramp).
    assert_eq!(log.iter().find(|e| e.1 != 0).unwrap().0, 6600);
    assert_eq!(
        outputs(&mut engine(None), 30_000),
        outputs(&mut engine(None), 30_000)
    );
    // Seed 0 draws randomized stage durations instead, so it is a different run.
    assert_ne!(
        outputs(&mut engine(Some(0)), 30_000),
        outputs(&mut engine(None), 30_000)
    );
}

#[test]
fn resets_replay_cleanly_and_warm_history_does_not_leak_between_sessions() {
    for seed in [None, Some(3)] {
        let mut fresh = engine(seed);
        let first = outputs(&mut fresh, 40_000);
        for how in 0..4 {
            let mut e = engine(seed);
            outputs(&mut e, 27_300);
            match how {
                // Source change and Stop/restart.
                0 | 1 => e.reset_temporal(),
                // The same seed again restarts the session.
                2 => e.configure_seed(seed.unwrap_or(0)).unwrap(),
                // A reduced-motion change resets, then restoring replays.
                _ => {
                    e.configure_automatic(true, true).unwrap();
                    let mut plain = MirrorEngine::new(W, H).unwrap();
                    for t in (0..=40_000).step_by(100) {
                        let input = frame(t);
                        let out = e.process_automatic_rgba(&input, t as f64).unwrap();
                        assert_eq!(out, plain.process_rgba(&input, MODE_MIRROR, 0).unwrap());
                        assert_eq!(e.haunt_strength(), 0);
                    }
                    e.configure_automatic(true, false).unwrap();
                }
            }
            if how == 2 && seed.is_none() {
                // configure_seed(0) is a seeded session, not the default one.
                assert_ne!(outputs(&mut e, 40_000), first);
            } else {
                assert_eq!(outputs(&mut e, 40_000), first, "{seed:?} {how}");
            }
        }
        // A different seed matches a reference built with that seed.
        let mut e = engine(seed);
        outputs(&mut e, 20_000);
        e.configure_seed(9).unwrap();
        let mut r = reference(Some(9));
        run(&mut e, &mut r, (0..=40_000).step_by(100), frame);
    }
}

#[test]
fn identical_configuration_calls_preserve_the_session() {
    let (mut a, mut b) = (engine(Some(2)), engine(Some(2)));
    for t in (0..=40_000).step_by(100) {
        if t % 7000 == 0 {
            a.configure_automatic(true, false).unwrap();
            a.configure_skull(false).unwrap();
            a.clear_head();
        }
        let input = frame(t);
        assert_eq!(
            a.process_automatic_rgba(&input, t as f64).unwrap(),
            b.process_automatic_rgba(&input, t as f64).unwrap(),
            "t={t}"
        );
    }
}

#[test]
fn recovery_continues_an_existing_event_and_never_selects_a_replacement() {
    let mut continued = 0;
    for stop in [11_000, 12_000, 13_000, 15_000, 20_000, 25_000] {
        let (mut e, mut r) = (engine(None), reference(None));
        let log = run(&mut e, &mut r, (0..=60_000).step_by(100), |t| {
            if t <= stop {
                frame(t)
            } else {
                still()
            }
        });
        assert!(log.iter().any(|e| e.2 == HauntState::Recovering));
        for pair in log.windows(2) {
            let (before, now) = (pair[0], pair[1]);
            // The selection frame is neutral, so a new event during recovery
            // would show a first positive frame after a Recovering frame.
            if now.1 != 0 && before.1 == 0 {
                assert_ne!(before.2, HauntState::Recovering, "stop {stop} {now:?}");
            }
            if now.2 == HauntState::Recovering && now.1 != 0 && before.1 == now.1 {
                continued += 1;
            }
        }
    }
    assert!(continued > 20, "{continued}");
}

#[test]
fn runtime_reduced_motion_returns_neutral_output_at_once() {
    let mut e = engine(Some(2));
    let mut r = reference(Some(2));
    run(&mut e, &mut r, (0..=14_000).step_by(100), frame);
    assert!(e.haunt_strength() > 0);
    e.configure_automatic(true, true).unwrap();
    assert_eq!((e.haunt_strength(), e.haunt_state()), (0, 0));
    for t in (14_100..=40_000).step_by(100) {
        let input = frame(t);
        assert_eq!(
            e.process_automatic_rgba(&input, t as f64).unwrap(),
            clear(&input)
        );
        assert_eq!(e.skull_opacity(), 0);
    }
}

#[test]
fn invalid_calls_leave_a_twin_identical_for_all_future_results() {
    let (mut a, mut b) = (engine(Some(2)), engine(Some(2)));
    for t in (0..=40_000).step_by(100) {
        if t >= 500 {
            let input = frame(t);
            let last = t as f64;
            for bad in [
                f64::NAN,
                f64::INFINITY,
                f64::NEG_INFINITY,
                -1.0,
                last + 0.5,
                last - 101.0,
                0.0,
                9_007_199_254_740_992.0,
            ] {
                assert!(a.process_automatic_rgba(&input, bad).is_err(), "{bad}");
            }
            assert!(a.process_automatic_rgba(&input[1..], last).is_err());
            assert!(a.process_automatic_rgba(&[], last).is_err());
            assert!(a.configure_temporal(601, 0).is_err());
            assert!(a.observe_head(0, 0, 0, 1, 900, last).is_err());
            assert!(a.process_rgba(&input, 3, 0).is_err());
        }
        let input = frame(t);
        assert_eq!(
            a.process_automatic_rgba(&input, t as f64).unwrap(),
            b.process_automatic_rgba(&input, t as f64).unwrap(),
            "t={t}"
        );
        assert_eq!(a.haunt_state(), b.haunt_state());
        assert_eq!(a.motion_activity(), b.motion_activity());
    }
}

#[test]
fn huge_jumps_and_the_maximum_timestamp_match_the_reference() {
    let (mut e, mut r) = (engine(Some(6)), reference(Some(6)));
    run(&mut e, &mut r, (0..=20_000).step_by(100), frame);
    let far = 9_007_199_254_700_000u64;
    let log = run(&mut e, &mut r, (far..far + 30_000).step_by(100), frame);
    // A jump replays no missed event: at most one run starts per 100 ms step.
    assert!(masks(&log).len() <= 3, "{:?}", masks(&log));
    let max = 9_007_199_254_740_991u64;
    run(&mut e, &mut r, [max, max].into_iter(), frame);
    assert!(e
        .process_automatic_rgba(&frame(0), 9_007_199_254_740_992.0)
        .is_err());
}

#[test]
fn events_are_separated_by_the_director_cooldown() {
    for seed in [None, Some(1), Some(2), Some(4)] {
        let (mut e, mut r) = (engine(seed), reference(seed));
        let log = run(&mut e, &mut r, (0..=90_000).step_by(100), frame);
        let mut last_end: Option<u64> = None;
        let mut selected = false;
        for pair in log.windows(2) {
            if pair[0].1 != 0 && pair[1].1 == 0 {
                last_end = Some(pair[1].0);
            }
            if pair[0].1 == 0 && pair[1].1 != 0 && selected {
                // First positive frame follows the selection frame by 100 ms.
                let gap = pair[1].0 - 100 - last_end.unwrap();
                assert!(gap >= 3000, "{seed:?} gap {gap}");
            }
            selected |= pair[1].1 != 0;
        }
        assert!(masks(&log).len() >= 5);
    }
}
