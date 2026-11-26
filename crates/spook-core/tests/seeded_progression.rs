use spook_core::{
    HauntProgression, HauntState, HauntTiming, HAUNT_FULL_STRENGTH, MAX_STAGE_MS, MIN_STAGE_MS,
};

fn in_range(t: HauntTiming) -> bool {
    [t.intro_ms, t.rise_ms, t.hold_ms, t.recovery_ms]
        .iter()
        .all(|ms| (MIN_STAGE_MS..=MAX_STAGE_MS).contains(ms))
}

#[test]
fn unseeded_timing_is_the_original_fixed_cycle() {
    let p = HauntProgression::new().unwrap();
    assert_eq!(p.timing(), HauntTiming::FIXED);
    assert_eq!(p.seed(), None);
}

#[test]
fn seeded_rise_reaches_full_strength_independently_of_frame_cadence() {
    for seed in 0..20 {
        let mut fine = HauntProgression::with_seed(seed).unwrap();
        let mut coarse = HauntProgression::with_seed(seed).unwrap();
        let timing = fine.timing();
        for p in [&mut fine, &mut coarse] {
            p.update(0, 10, false).unwrap();
            p.update(500, 10, false).unwrap();
            p.update(timing.intro_ms, 10, false).unwrap();
        }
        let end = timing.intro_ms + timing.rise_ms;
        let expected = coarse.update(end, 10, false).unwrap().strength;
        let mut actual = 0;
        for now in timing.intro_ms + 1..=end {
            actual = fine.update(now, 10, false).unwrap().strength;
        }
        assert_eq!(expected, HAUNT_FULL_STRENGTH);
        assert_eq!(actual, expected, "seed {seed}");
    }
}

#[test]
fn invalid_duration_changes_preserve_the_running_state_machine() {
    use spook_core::{HauntConfig, HauntError, HauntMachine};
    let mut a = HauntMachine::new(HauntConfig::default()).unwrap();
    let mut b = HauntMachine::new(HauntConfig::default()).unwrap();
    for now in [0, 500, 1000] {
        assert_eq!(a.update(now, 1000), b.update(now, 1000));
    }
    for durations in [(0, 10, 10), (10, 120_000, 10), (10, 10, u64::MAX)] {
        assert_eq!(
            a.set_durations(durations.0, durations.1, durations.2),
            Err(HauntError::InvalidConfig)
        );
    }
    for now in [7000, 11_000, 21_000] {
        assert_eq!(a.update(now, 0), b.update(now, 0));
    }
    assert!(a.set_durations(1, 119_999, 1).is_ok());
}

#[test]
fn seeds_give_bounded_deterministic_and_varied_timing() {
    let mut distinct = std::collections::HashSet::new();
    for seed in 0..200u32 {
        let a = HauntProgression::with_seed(seed).unwrap();
        let b = HauntProgression::with_seed(seed).unwrap();
        assert_eq!(a.timing(), b.timing());
        assert_eq!(a.seed(), Some(seed));
        assert!(in_range(a.timing()), "{seed} {:?}", a.timing());
        distinct.insert(a.timing().intro_ms);
    }
    assert!(
        distinct.len() > 50,
        "only {} distinct openings",
        distinct.len()
    );
    // Both ends of the range are reachable.
    let all: Vec<_> = (0..5000u32)
        .map(|s| HauntProgression::with_seed(s).unwrap().timing().rise_ms)
        .collect();
    assert!(all.iter().any(|&m| m < MIN_STAGE_MS + 200));
    assert!(all.iter().any(|&m| m > MAX_STAGE_MS - 200));
}

/// Moving input every 50 ms, from a fresh progression.
fn drive(p: &mut HauntProgression, from: u64, to: u64, activity: u16) -> Vec<(u64, u16)> {
    (from..=to)
        .step_by(50)
        .map(|t| (t, p.update(t, activity, false).unwrap().strength))
        .collect()
}

#[test]
fn seeded_cycle_is_clear_then_smooth_then_returns_without_jumps() {
    let mut p = HauntProgression::with_seed(7).unwrap();
    let timing = p.timing();
    let mut samples = drive(&mut p, 0, timing.intro_ms + timing.rise_ms + 3000, 10);
    // Clear until the drawn opening ends (plus the 500 ms debounce window).
    for &(t, s) in &samples {
        if t <= timing.intro_ms {
            assert_eq!(s, 0, "t={t}");
        }
    }
    let end = samples.last().unwrap().0;
    assert_eq!(samples.last().unwrap().1, HAUNT_FULL_STRENGTH);
    // Stillness: the grace holds full strength, then it returns slowly.
    samples.extend(drive(
        &mut p,
        end + 50,
        end + timing.hold_ms + timing.recovery_ms + 2000,
        0,
    ));
    let mut peak = 0;
    for pair in samples.windows(2) {
        let step = pair[0].1.abs_diff(pair[1].1);
        peak = peak.max(step);
        // 50 ms steps on a 4 s curve with smoothstep slope 1.5: at most 113.
        assert!(step <= 120, "jump {step} at {:?}", pair);
    }
    assert!(peak > 0);
    assert_eq!(samples.last().unwrap().1, 0);
    // The return is gradual: it takes at least the minimum stage time.
    let held = samples
        .iter()
        .rposition(|&(_, s)| s == HAUNT_FULL_STRENGTH)
        .unwrap();
    let gone = samples
        .iter()
        .position(|&(t, s)| t > samples[held].0 && s == 0)
        .unwrap();
    assert!(samples[gone].0 - samples[held].0 >= MIN_STAGE_MS);
}

#[test]
fn smoothstep_starts_and_ends_gently() {
    let mut p = HauntProgression::with_seed(3).unwrap();
    let t = p.timing();
    let samples = drive(&mut p, 0, t.intro_ms + t.rise_ms + 1000, 10);
    let rise: Vec<_> = samples
        .iter()
        .filter(|&&(_, s)| s > 0 && s < HAUNT_FULL_STRENGTH)
        .collect();
    let first = rise.first().unwrap().1;
    let mid = rise[rise.len() / 2].1;
    let steps: Vec<u16> = rise.windows(2).map(|w| w[1].1 - w[0].1).collect();
    assert!(first < 60, "gentle start {first}");
    assert!(
        steps[steps.len() / 2] > steps[0] * 3,
        "middle faster than the start"
    );
    assert!(
        steps[steps.len() / 2] > *steps.last().unwrap() * 3,
        "end slows again"
    );
    assert!((2000..4000).contains(&mid));
}

#[test]
fn a_finished_visit_draws_new_timing_and_replays_after_reset() {
    let mut p = HauntProgression::with_seed(11).unwrap();
    let first = p.timing();
    let mut t = 0;
    let mut timings = vec![first];
    let mut active = true;
    // Alternate presence and long stillness through several visits.
    for _ in 0..4 {
        for _ in 0..(30_000 / 50) {
            p.update(t, if active { 10 } else { 0 }, false).unwrap();
            t += 50;
        }
        active = !active;
        if timings.last() != Some(&p.timing()) {
            timings.push(p.timing());
        }
    }
    assert!(timings.len() >= 2, "no new draw: {timings:?}");
    assert!(timings.iter().all(|&x| in_range(x)));
    assert_ne!(timings[0], timings[1]);
    p.reset();
    assert_eq!(p.timing(), first, "reset replays the same seeded sequence");
    assert_eq!(p.update(0, 0, false).unwrap().state, HauntState::Idle);
}

#[test]
fn huge_jumps_and_regressions_do_not_overflow_or_change_state() {
    let mut p = HauntProgression::with_seed(1).unwrap();
    p.update(10, 10, false).unwrap();
    let far = u64::MAX - 5;
    assert!(p.update(far, 10, false).is_ok());
    assert!(p.update(far - 1, 10, false).is_err());
    assert!(p.update(far, 2000, false).is_err());
    let t = p.timing();
    assert!(p.update(far, 10, false).is_ok());
    assert_eq!(p.timing(), t);
}

#[test]
fn reduced_motion_clears_seeded_strength_immediately() {
    let mut p = HauntProgression::with_seed(5).unwrap();
    let t = p.timing();
    drive(&mut p, 0, t.intro_ms + t.rise_ms, 10);
    assert_eq!(
        p.update(t.intro_ms + t.rise_ms + 50, 10, true)
            .unwrap()
            .strength,
        0
    );
}
