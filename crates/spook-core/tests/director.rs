use spook_core::{
    DirectorConfig, DirectorError, DirectorInput, DirectorSnapshot, HauntDirector, HauntLevel,
    HauntState, ECHO, LAG, SKULL,
};

const NEUTRAL: DirectorSnapshot = DirectorSnapshot {
    layers: 0,
    strength: 0,
    delay_ms: 0,
    trail_strength: 0,
};

fn input(state: HauntState, strength: u16) -> DirectorInput {
    DirectorInput {
        level: HauntLevel {
            state,
            strength,
            activity: 500,
        },
        head_available: true,
        reduced_motion: false,
        quality_budget: 2,
    }
}

fn full() -> DirectorInput {
    input(HauntState::Haunted, 6000)
}

fn director(seed: u32) -> HauntDirector {
    HauntDirector::new(seed, DirectorConfig::default()).unwrap()
}

fn with_layers(seed: u32, enabled_layers: u8) -> HauntDirector {
    HauntDirector::new(seed, DirectorConfig { enabled_layers }).unwrap()
}

/// Starts an event at `t0` and measures its duration with throw-away clones.
/// Returns the director (event active) and the duration in ms.
fn start(mut d: HauntDirector, t0: u64) -> (HauntDirector, u64) {
    assert_eq!(d.update(t0, full()).unwrap(), NEUTRAL); // fade position 0
    let mut found = None;
    for dur in 4000..=8000u64 {
        let mut probe = d.clone();
        if probe.update(t0 + dur, full()).unwrap() == NEUTRAL {
            found = Some(dur);
            break;
        }
    }
    (d, found.expect("event must end within 8000 ms"))
}

fn mask_at(d: &HauntDirector, t: u64) -> u8 {
    d.clone().update(t, full()).unwrap().layers
}

/// Runs one event per iteration, separated by the full cooldown, and returns masks.
fn event_masks(mut d: HauntDirector, count: usize, input: DirectorInput) -> Vec<u8> {
    let mut t = 0u64;
    let mut masks = Vec::new();
    while masks.len() < count {
        d.update(t, input).unwrap();
        let mid = d.clone().update(t + 500, input).unwrap().layers;
        assert_ne!(mid, 0, "an event must start");
        masks.push(mid);
        t += 20_000;
        d.update(t, input).unwrap(); // completes (neutral)
        t += 20_000;
    }
    masks
}

#[test]
fn fade_boundaries_use_independent_integer_values() {
    let (d, dur) = start(director(7), 1000);
    assert!((4000..=8000).contains(&dur));
    let at = |offset: u64| d.clone().update(1000 + offset, full()).unwrap();
    // Strength 6000 and envelope permille e give exactly 6 * e.
    assert_eq!(at(0), NEUTRAL);
    assert_eq!(at(1).strength, 6);
    assert_eq!(at(999).strength, 5994);
    assert_eq!(at(1000).strength, 6000);
    assert_eq!(at(1001).strength, 6000);
    assert_eq!(at(dur - 1000).strength, 6000);
    assert_eq!(at(dur - 999).strength, 5994);
    assert_eq!(at(dur - 1).strength, 6);
    assert_eq!(at(dur), NEUTRAL);
}

#[test]
fn strength_rounds_half_up_and_parameters_floor() {
    let (d, _) = start(director(7), 0);
    let half = input(HauntState::Haunted, 3000);
    // envelope 500 * 3000 = 1_500_000 -> exactly 1500.
    assert_eq!(d.clone().update(500, half).unwrap().strength, 1500);
    // envelope 1 * 1 / 1000 rounds to 0 -> neutral; 500 * 1 rounds up to 1.
    let tiny = input(HauntState::Haunted, 1);
    assert_eq!(d.clone().update(1, tiny).unwrap(), NEUTRAL);
    assert_eq!(d.clone().update(500, tiny).unwrap().strength, 1);
    assert_eq!(d.clone().update(499, tiny).unwrap(), NEUTRAL);
}

#[test]
fn parameters_scale_by_strength_over_6000_and_respect_ranges() {
    for seed in 0..40 {
        let (d, dur) = start(director(seed), 0);
        let s = d.clone().update(1500, full()).unwrap();
        assert_eq!(s.strength, 6000);
        assert_eq!(s.delay_ms == 0, s.layers & LAG == 0);
        assert_eq!(s.trail_strength == 0, s.layers & ECHO == 0);
        if s.layers & LAG != 0 {
            assert!((150..=450).contains(&s.delay_ms));
        }
        if s.layers & ECHO != 0 {
            assert!((48..=128).contains(&s.trail_strength));
        }
        // Half level strength halves (floors) both requests.
        let half = d
            .clone()
            .update(dur / 2, input(HauntState::Haunted, 3000))
            .unwrap();
        assert_eq!(half.strength, 3000);
        assert_eq!(
            u32::from(half.delay_ms),
            u32::from(s.delay_ms) * 3000 / 6000
        );
        assert_eq!(
            u32::from(half.trail_strength),
            u32::from(s.trail_strength) * 3000 / 6000
        );
    }
}

#[test]
fn durations_cover_the_documented_range() {
    let durations: Vec<u64> = (0..60).map(|s| start(director(s), 0).1).collect();
    assert!(durations.iter().all(|d| (4000..=8000).contains(d)));
    assert!(durations.iter().min().unwrap() < &5000);
    assert!(durations.iter().max().unwrap() > &7000);
}

#[test]
fn minimum_selection_strength_is_1500() {
    // An event begun at t=0 is already fading in at t=1; a late start is not.
    let mut below = director(3);
    below.update(0, input(HauntState::Haunted, 1499)).unwrap();
    assert_eq!(below.update(1, full()).unwrap(), NEUTRAL);
    let mut at = director(3);
    at.update(0, input(HauntState::Haunted, 1500)).unwrap();
    assert_eq!(at.update(1, full()).unwrap().strength, 6);
}

#[test]
fn neutral_phases_zero_strength_and_reduced_motion_never_start() {
    for state in [HauntState::Idle, HauntState::PresenceDetected] {
        let mut d = director(3);
        d.update(0, input(state, 6000)).unwrap();
        assert_eq!(d.update(1, full()).unwrap(), NEUTRAL, "{state:?}");
    }
    let mut d = director(3);
    d.update(0, input(HauntState::Haunted, 0)).unwrap();
    assert_eq!(d.update(1, full()).unwrap(), NEUTRAL);
    let mut d = director(3);
    let mut reduced = full();
    reduced.reduced_motion = true;
    d.update(0, reduced).unwrap();
    assert_eq!(d.update(1, full()).unwrap(), NEUTRAL);
    // Recovering never starts one.
    let mut d = director(3);
    d.update(0, input(HauntState::Recovering, 6000)).unwrap();
    assert_eq!(d.update(1, full()).unwrap(), NEUTRAL);
    // Transforming does.
    let mut d = director(3);
    d.update(0, input(HauntState::Transforming, 6000)).unwrap();
    assert_eq!(d.update(1, full()).unwrap().strength, 6);
}

#[test]
fn active_event_is_cancelled_immediately_and_not_resumed() {
    let neutral_inputs = [
        input(HauntState::Idle, 6000),
        input(HauntState::PresenceDetected, 6000),
        input(HauntState::Haunted, 0),
        DirectorInput {
            reduced_motion: true,
            ..full()
        },
    ];
    for (i, bad) in neutral_inputs.into_iter().enumerate() {
        let (mut d, _) = start(director(5), 0);
        assert_ne!(d.update(1500, full()).unwrap(), NEUTRAL);
        assert_eq!(d.update(1600, bad).unwrap(), NEUTRAL, "case {i}");
        // Same instant and a later one: cancelled, in cooldown, no resume.
        assert_eq!(d.update(1600, full()).unwrap(), NEUTRAL);
        assert_eq!(d.update(1700, full()).unwrap(), NEUTRAL);
    }
}

#[test]
fn recovery_scales_active_event_and_starts_no_replacement() {
    let (mut d, dur) = start(director(5), 0);
    let r = input(HauntState::Recovering, 3000);
    assert_eq!(d.update(1500, r).unwrap().strength, 3000);
    assert_eq!(d.update(1500, r).unwrap().strength, 3000);
    // Zero strength while recovering cancels.
    let mut z = d.clone();
    assert_eq!(
        z.update(1600, input(HauntState::Recovering, 0)).unwrap(),
        NEUTRAL
    );
    // After completion in Recovering nothing new starts, however late.
    assert_eq!(d.update(dur, r).unwrap(), NEUTRAL);
    for t in [dur + 3000, dur + 8000, dur + 50_000] {
        assert_eq!(d.update(t, r).unwrap(), NEUTRAL);
        assert_eq!(d.update(t + 1, r).unwrap(), NEUTRAL);
    }
}

#[test]
fn global_cooldown_boundary_2999_3000() {
    // Pick a seed whose first event leaves a disjoint candidate available.
    let seed = (0..100)
        .find(|&s| {
            let (d, _) = start(director(s), 0);
            mask_at(&d, 1500) != 7
        })
        .unwrap();
    let (mut d, dur) = start(director(seed), 0);
    assert_eq!(d.update(dur, full()).unwrap(), NEUTRAL);
    let mut early = d.clone();
    early.update(dur + 2999, full()).unwrap();
    assert_eq!(early.update(dur + 3000, full()).unwrap(), NEUTRAL);
    let mut on_time = d.clone();
    on_time.update(dur + 3000, full()).unwrap();
    assert_eq!(on_time.update(dur + 3001, full()).unwrap().strength, 6);
    // Repeated equal timestamps do not bypass the window.
    for _ in 0..5 {
        assert_eq!(d.update(dur + 100, full()).unwrap(), NEUTRAL);
    }
}

#[test]
fn layer_cooldown_boundary_7999_8000() {
    let (mut d, dur) = start(with_layers(9, LAG), 0);
    assert_eq!(d.update(dur, full()).unwrap(), NEUTRAL);
    let mut early = d.clone();
    early.update(dur + 7999, full()).unwrap();
    assert_eq!(early.update(dur + 8000, full()).unwrap(), NEUTRAL);
    let mut on_time = d.clone();
    on_time.update(dur + 8000, full()).unwrap();
    let s = on_time.update(dur + 8001, full()).unwrap();
    assert_eq!((s.layers, s.strength), (LAG, 6));
    // Nothing starts anywhere inside the window.
    for t in (dur + 3000..dur + 8000).step_by(250) {
        assert_eq!(d.update(t, full()).unwrap(), NEUTRAL);
    }
}

#[test]
fn changing_other_bits_cannot_evade_a_layer_cooldown() {
    for seed in 0..30 {
        let (mut d, dur) = start(director(seed), 0);
        let used = mask_at(&d, 1500);
        d.update(dur, full()).unwrap();
        let mut t = dur + 3000;
        while t < dur + 8000 {
            let s = d.update(t, full()).unwrap();
            assert_eq!(s.layers & used, 0, "seed {seed} reused {used} at {t}");
            t += 100;
        }
    }
}

#[test]
fn cancelled_events_also_start_cooldowns_once() {
    let (mut d, _) = start(with_layers(2, ECHO), 0);
    assert_ne!(d.update(2000, full()).unwrap(), NEUTRAL);
    let mut idle = full();
    idle.level.state = HauntState::Idle;
    assert_eq!(d.update(2500, idle).unwrap(), NEUTRAL);
    // Neutral updates do not refresh the cooldown start (2500).
    for t in [3000u64, 5000, 9000, 10_000] {
        d.update(t, idle).unwrap();
    }
    assert_eq!(d.update(10_499, full()).unwrap(), NEUTRAL);
    assert_eq!(d.update(10_500, full()).unwrap(), NEUTRAL); // starts, fade 0
    assert_eq!(d.update(10_501, full()).unwrap().strength, 6);
}

#[test]
fn budgets_limit_combinations() {
    let mut zero = full();
    zero.quality_budget = 0;
    let mut one = full();
    one.quality_budget = 1;
    for seed in 0..30 {
        let mut d = director(seed);
        d.update(0, zero).unwrap();
        assert_eq!(d.update(500, zero).unwrap(), NEUTRAL);
    }
    let masks: Vec<u8> = (0..30)
        .flat_map(|s| event_masks(director(s), 6, one))
        .collect();
    assert!(masks.iter().all(|m| [1, 2, 3, 4].contains(m)), "{masks:?}");
    for allowed in [1u8, 2, 3, 4] {
        assert!(masks.contains(&allowed), "mask {allowed} never chosen");
    }
    let masks: Vec<u8> = (0..30)
        .flat_map(|s| event_masks(director(s), 6, full()))
        .collect();
    for m in 1..=7u8 {
        assert!(masks.contains(&m), "mask {m} never chosen with budget 2");
    }
}

#[test]
fn enabled_bits_and_head_availability_are_respected() {
    for enabled in 0..=7u8 {
        let mut input = full();
        input.head_available = false;
        for seed in 0..10 {
            let mut d = with_layers(seed, enabled);
            let mut t = 0;
            for _ in 0..8 {
                let s = d.update(t, full()).unwrap();
                assert_eq!(s.layers & !enabled, 0);
                t += 1500;
                let s = d.update(t, full()).unwrap();
                assert_eq!(s.layers & !enabled, 0);
                t += 12_000;
            }
            let mut d = with_layers(seed, enabled);
            for step in 0..40u64 {
                let s = d.update(step * 1000, input).unwrap();
                assert_eq!(s.layers & SKULL, 0, "no head, no skull");
            }
        }
    }
    let mut d = with_layers(1, 0);
    for t in 0..20u64 {
        assert_eq!(d.update(t * 1000, full()).unwrap(), NEUTRAL);
    }
}

#[test]
fn skull_only_with_no_head_never_starts() {
    let mut d = with_layers(4, SKULL);
    let mut no_head = full();
    no_head.head_available = false;
    for t in (0..30_000u64).step_by(500) {
        assert_eq!(d.update(t, no_head).unwrap(), NEUTRAL);
    }
}

#[test]
fn head_loss_and_budget_shrink_cancel_a_skull_event_at_once() {
    let seed = (0..200)
        .find(|&s| mask_at(&start(director(s), 0).0, 1500) & SKULL != 0)
        .expect("a skull seed");
    let (d, _) = start(director(seed), 0);
    let mut lost = d.clone();
    let mut no_head = full();
    no_head.head_available = false;
    assert_ne!(lost.update(1500, full()).unwrap(), NEUTRAL);
    assert_eq!(lost.update(1501, no_head).unwrap(), NEUTRAL);
    // The head returning does not resurrect it or bypass the cooldown.
    assert_eq!(lost.update(1501, full()).unwrap(), NEUTRAL);
    assert_eq!(lost.update(2500, full()).unwrap(), NEUTRAL);

    let mask = mask_at(&d, 1500);
    let mut shrunk = d.clone();
    let mut budget = full();
    budget.quality_budget = cost_of(mask) - 1;
    assert_eq!(shrunk.update(1501, budget).unwrap(), NEUTRAL);
    assert_eq!(shrunk.update(1502, full()).unwrap(), NEUTRAL);
}

fn cost_of(mask: u8) -> u8 {
    u8::from(mask & (LAG | ECHO) != 0) + u8::from(mask & SKULL != 0)
}

#[test]
fn no_immediate_repeat_when_alternatives_and_sole_candidate_allowed() {
    for seed in 0..20 {
        let masks = event_masks(director(seed), 25, full());
        assert!(
            masks.windows(2).all(|w| w[0] != w[1]),
            "seed {seed}: {masks:?}"
        );
    }
    let masks = event_masks(with_layers(1, SKULL), 6, full());
    assert_eq!(masks, vec![SKULL; 6]);
}

#[test]
fn recent_history_lowers_repeats_of_older_masks() {
    // Lag, echo and their combination are eligible; the previous mask is excluded.
    let masks = event_masks(with_layers(5, LAG | ECHO), 3, full());
    assert!(masks.windows(2).all(|w| w[0] != w[1]));
    // Over many seeds, the share of masks seen within the last four events is
    // lower than uniform choice would give (7 masks, 3 other slots of 4).
    let mut repeats_within_four = 0;
    let mut total = 0;
    for seed in 0..40 {
        let m = event_masks(director(seed), 20, full());
        for i in 4..m.len() {
            total += 1;
            if m[i - 4..i].contains(&m[i]) {
                repeats_within_four += 1;
            }
        }
    }
    // Uniform over the six non-previous masks would repeat a recent one in
    // roughly 3/6 of draws; halved weights must reduce that below 1/2.
    assert!(
        repeats_within_four * 2 < total,
        "{repeats_within_four}/{total}"
    );
}

#[test]
fn seeds_replay_exactly_and_differ_across_seeds() {
    let a = event_masks(director(11), 30, full());
    assert_eq!(a, event_masks(director(11), 30, full()));
    let sequences: Vec<Vec<u8>> = (0..12)
        .map(|s| event_masks(director(s), 12, full()))
        .collect();
    let mut unique = sequences.clone();
    unique.sort();
    unique.dedup();
    assert!(
        unique.len() >= 10,
        "only {} distinct sequences",
        unique.len()
    );
}

#[test]
fn first_events_match_independent_seed_vectors() {
    // Independently calculated SplitMix64 tickets and the following three draws.
    for (seed, layers, duration, delay_ms, trail_strength) in [
        (0, ECHO | SKULL, 6359, 0, 109),
        (7, LAG | ECHO | SKULL, 7166, 388, 126),
        (42, LAG | ECHO | SKULL, 7814, 227, 48),
    ] {
        let mut d = director(seed);
        assert_eq!(d.update(0, full()).unwrap(), NEUTRAL);
        assert_eq!(
            d.update(1000, full()).unwrap(),
            DirectorSnapshot {
                layers,
                strength: 6000,
                delay_ms,
                trail_strength,
            }
        );
        assert_eq!(d.update(duration - 1, full()).unwrap().strength, 6);
        assert_eq!(d.update(duration, full()).unwrap(), NEUTRAL);
    }
}

#[test]
fn invalid_calls_leave_later_results_identical_to_an_untouched_twin() {
    let mut touched = director(21);
    let mut twin = director(21);
    let mut bad_budget = full();
    bad_budget.quality_budget = 3;
    let mut bad_strength = full();
    bad_strength.level.strength = 6001;
    let mut bad_activity = full();
    bad_activity.level.activity = 1001;
    for t in (0..60_000u64).step_by(700) {
        assert_eq!(
            touched.update(t, bad_budget),
            Err(DirectorError::QualityBudget)
        );
        assert_eq!(
            touched.update(t, bad_strength),
            Err(DirectorError::Strength)
        );
        assert_eq!(
            touched.update(t, bad_activity),
            Err(DirectorError::Activity)
        );
        assert_eq!(
            touched.update(t + 5, bad_budget),
            Err(DirectorError::QualityBudget)
        );
        let a = touched.update(t, full()).unwrap();
        if t > 0 {
            assert_eq!(
                touched.update(t - 1, full()),
                Err(DirectorError::TimeRegressed)
            );
        }
        assert_eq!(a, twin.update(t, full()).unwrap(), "t={t}");
    }
    // An invalid call must not set the clock either (time far in the future).
    let mut d = director(21);
    assert!(d.update(u64::MAX, bad_budget).is_err());
    assert!(d.update(0, full()).is_ok());
}

#[test]
fn time_must_not_regress_but_equal_time_is_idempotent() {
    let mut d = director(2);
    d.update(5000, full()).unwrap();
    assert_eq!(d.update(4999, full()), Err(DirectorError::TimeRegressed));
    let first = d.update(5500, full()).unwrap();
    for _ in 0..10 {
        assert_eq!(d.update(5500, full()).unwrap(), first);
    }
    // Repeated equal timestamps do not fill history: a twin that made one
    // call selects the same next event.
    let mut twin = director(2);
    twin.update(5000, full()).unwrap();
    twin.update(5500, full()).unwrap();
    for t in [20_000u64, 40_000, 60_000] {
        assert_eq!(
            d.update(t, full()).unwrap(),
            twin.update(t, full()).unwrap()
        );
    }
}

#[test]
fn config_validation_and_errors_display() {
    assert_eq!(
        HauntDirector::new(0, DirectorConfig { enabled_layers: 8 }).unwrap_err(),
        DirectorError::UnknownLayers
    );
    assert_eq!(DirectorConfig::default().enabled_layers, LAG | ECHO | SKULL);
    assert_eq!((LAG, ECHO, SKULL), (1, 2, 4));
    for e in [
        DirectorError::UnknownLayers,
        DirectorError::QualityBudget,
        DirectorError::Strength,
        DirectorError::Activity,
        DirectorError::TimeRegressed,
    ] {
        assert!(!e.to_string().is_empty());
        let _: &dyn std::error::Error = &e;
    }
}

#[test]
fn reset_replays_the_initial_sequence() {
    let mut d = director(33);
    let before = event_masks(d.clone(), 8, full());
    for t in (0..100_000u64).step_by(900) {
        d.update(t, full()).unwrap();
    }
    d.reset();
    assert_eq!(event_masks(d.clone(), 8, full()), before);
    // The clock is cleared: time may restart at zero.
    assert!(d.update(0, full()).is_ok());
    let mut fresh = director(33);
    for t in (0..60_000u64).step_by(400) {
        assert_eq!(
            d.update(t, full()).unwrap(),
            fresh.update(t, full()).unwrap()
        );
    }
}

#[test]
fn huge_jumps_complete_one_event_and_consume_no_extra_randomness() {
    let (mut jumped, dur) = start(director(8), 0);
    assert_eq!(jumped.update(1_000_000_000_000, full()).unwrap(), NEUTRAL);
    assert_eq!(jumped.update(1_000_000_000_000, full()).unwrap(), NEUTRAL);
    // Cooldown begins at the observed completion time, not the deadline.
    assert_eq!(jumped.update(1_000_000_002_999, full()).unwrap(), NEUTRAL);
    let (mut natural, _) = start(director(8), 0);
    assert_eq!(natural.update(dur, full()).unwrap(), NEUTRAL);
    // Both have drawn the same randomness: the next events match.
    let base_j = 1_000_000_000_000u64 + 20_000;
    let base_n = dur + 20_000;
    jumped.update(base_j, full()).unwrap();
    natural.update(base_n, full()).unwrap();
    for off in [500u64, 1500, 3000] {
        assert_eq!(
            jumped.update(base_j + off, full()).unwrap(),
            natural.update(base_n + off, full()).unwrap()
        );
    }
}

#[test]
fn u64_max_is_supported() {
    let mut d = director(6);
    assert_eq!(d.update(u64::MAX, full()).unwrap(), NEUTRAL);
    assert_eq!(d.update(u64::MAX, full()).unwrap(), NEUTRAL);
    let mut d = director(6);
    d.update(u64::MAX - 10, full()).unwrap();
    assert_eq!(d.update(u64::MAX, full()).unwrap().strength, 60);
    let (mut d, _) = start(director(6), 0);
    assert_eq!(d.update(u64::MAX, full()).unwrap(), NEUTRAL);
    assert_eq!(d.update(u64::MAX, full()).unwrap(), NEUTRAL);
}

#[test]
fn director_storage_is_a_small_fixed_object() {
    // A size check shows the metadata is small; it does not prove that
    // update() never allocates.
    assert!(std::mem::size_of::<HauntDirector>() < 512);
}

/// Small deterministic generator for bounded input sequences.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn generated_sequences_keep_ranges_cost_and_determinism() {
    // 120 seeds x 400 updates each with varied phases, heads, budgets and
    // jumps up to 20 s. A bounded sample, not a proof.
    let states = [
        HauntState::Idle,
        HauntState::PresenceDetected,
        HauntState::Transforming,
        HauntState::Haunted,
        HauntState::Haunted,
        HauntState::Recovering,
    ];
    for seed in 0..120u32 {
        let mut rng = Lcg(u64::from(seed) * 7919 + 1);
        let enabled = (rng.next() % 8) as u8;
        let mut a = with_layers(seed, enabled);
        let mut b = with_layers(seed, enabled);
        let mut now = 0u64;
        for _ in 0..400 {
            now += rng.next() % 20_000;
            let level = HauntLevel {
                state: states[(rng.next() % 6) as usize],
                strength: (rng.next() % 6001) as u16,
                activity: (rng.next() % 1001) as u16,
            };
            let inp = DirectorInput {
                level,
                head_available: !rng.next().is_multiple_of(3),
                reduced_motion: rng.next().is_multiple_of(8),
                quality_budget: (rng.next() % 3) as u8,
            };
            let s = a.update(now, inp).unwrap();
            assert_eq!(s, b.update(now, inp).unwrap());
            assert!(s.strength <= 6000 && s.delay_ms <= 450 && s.trail_strength <= 128);
            assert_eq!(s.layers & !enabled, 0);
            assert_eq!(s.layers == 0, s.strength == 0);
            if s.layers == 0 {
                assert_eq!((s.delay_ms, s.trail_strength), (0, 0));
            } else {
                assert!(cost_of(s.layers) <= inp.quality_budget);
                assert!(!inp.reduced_motion && s.strength <= level.strength);
                if s.layers & SKULL != 0 {
                    assert!(inp.head_available);
                }
                if s.layers & LAG == 0 {
                    assert_eq!(s.delay_ms, 0);
                }
                if s.layers & ECHO == 0 {
                    assert_eq!(s.trail_strength, 0);
                }
            }
        }
    }
}
