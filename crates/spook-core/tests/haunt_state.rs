//! State-machine tests with literal timelines on the default configuration
//! (enter 50, leave 20, debounce 500, transform 4000, stillness 10000,
//! recovery 3000), plus bounded seeded generated sequences.

use spook_core::{HauntConfig, HauntError, HauntMachine, HauntSnapshot, HauntState};
use HauntState::{Haunted, Idle, PresenceDetected, Recovering, Transforming};

fn machine() -> HauntMachine {
    HauntMachine::new(HauntConfig::default()).unwrap()
}

fn snap(state: HauntState, elapsed: u64, active: bool) -> HauntSnapshot {
    HauntSnapshot {
        state,
        state_elapsed_ms: elapsed,
        active,
    }
}

/// Drives `m` through Idle -> Presence (0) -> Transforming (500) ->
/// Haunted (4500), last active at 4500.
fn haunted() -> HauntMachine {
    let mut m = machine();
    m.update(0, 60).unwrap();
    m.update(500, 60).unwrap();
    m.update(4500, 60).unwrap();
    assert_eq!(m.state(), Haunted);
    m
}

#[test]
fn default_config_matches_the_specification() {
    let c = HauntConfig::default();
    assert_eq!(
        (c.enter_activity, c.leave_activity, c.debounce_ms),
        (50, 20, 500)
    );
    assert_eq!(
        (c.transform_ms, c.stillness_ms, c.recovery_ms),
        (4000, 10_000, 3000)
    );
}

#[test]
fn full_cycle_with_boundaries_just_before_and_at_each_duration() {
    let mut m = machine();
    assert_eq!(m.update(0, 60).unwrap(), snap(PresenceDetected, 0, true));
    assert_eq!(
        m.update(499, 60).unwrap(),
        snap(PresenceDetected, 499, true)
    );
    assert_eq!(m.update(500, 60).unwrap(), snap(Transforming, 0, true));
    assert_eq!(m.update(4499, 60).unwrap(), snap(Transforming, 3999, true));
    assert_eq!(m.update(4500, 60).unwrap(), snap(Haunted, 0, true));
    assert_eq!(m.update(9000, 60).unwrap(), snap(Haunted, 4500, true));
    // Last active at 9000: grace holds at 9999 ms of stillness, ends at 10000.
    assert_eq!(m.update(18_999, 0).unwrap(), snap(Haunted, 14_499, false));
    assert_eq!(m.update(19_000, 0).unwrap(), snap(Recovering, 0, false));
    assert_eq!(m.update(21_999, 0).unwrap(), snap(Recovering, 2999, false));
    assert_eq!(m.update(22_000, 0).unwrap(), snap(Idle, 0, false));
    assert_eq!(m.update(90_000, 0).unwrap(), snap(Idle, 68_000, false));
}

#[test]
fn hysteresis_latch_sets_at_enter_clears_at_leave_and_retains_between() {
    let mut m = machine();
    assert_eq!(m.update(0, 49).unwrap(), snap(Idle, 0, false));
    assert_eq!(m.update(1, 30).unwrap(), snap(Idle, 1, false));
    assert_eq!(m.update(2, 50).unwrap(), snap(PresenceDetected, 0, true));
    // Between the thresholds an active latch stays active.
    assert_eq!(m.update(3, 21).unwrap(), snap(PresenceDetected, 1, true));
    assert_eq!(m.update(4, 30).unwrap(), snap(PresenceDetected, 2, true));
    assert_eq!(m.update(5, 20).unwrap(), snap(Idle, 0, false));
    // Back in Idle the latch is false and 21 does not re-arm it.
    assert_eq!(m.update(6, 21).unwrap(), snap(Idle, 1, false));
}

#[test]
fn interrupted_debounce_returns_to_idle_and_must_start_over() {
    let mut m = machine();
    m.update(0, 60).unwrap();
    assert_eq!(m.update(300, 0).unwrap(), snap(Idle, 0, false));
    assert_eq!(m.update(900, 60).unwrap(), snap(PresenceDetected, 0, true));
    assert_eq!(
        m.update(1399, 60).unwrap(),
        snap(PresenceDetected, 499, true)
    );
    assert_eq!(m.update(1400, 60).unwrap(), snap(Transforming, 0, true));
}

#[test]
fn stillness_ends_transforming_and_has_priority_over_transform_completion() {
    // Last active at 500 (when Transforming began).
    let mut early = machine();
    early.update(0, 60).unwrap();
    early.update(500, 60).unwrap();
    // 9999 ms of stillness: transform time elapsed, so Haunted.
    assert_eq!(early.update(10_499, 0).unwrap(), snap(Haunted, 0, false));

    let mut m = machine();
    m.update(0, 60).unwrap();
    m.update(500, 60).unwrap();
    // 10000 ms of stillness and 10000 ms in state: stillness wins.
    assert_eq!(m.update(10_500, 0).unwrap(), snap(Recovering, 0, false));
}

#[test]
fn transforming_stays_until_the_transform_time_even_when_still_briefly() {
    let mut m = machine();
    m.update(0, 60).unwrap();
    m.update(500, 60).unwrap();
    assert_eq!(m.update(2000, 0).unwrap(), snap(Transforming, 1500, false));
    assert_eq!(m.update(4499, 0).unwrap(), snap(Transforming, 3999, false));
    assert_eq!(m.update(4500, 0).unwrap(), snap(Haunted, 0, false));
}

#[test]
fn renewed_motion_restarts_the_stillness_grace() {
    let mut m = haunted();
    assert_eq!(m.update(14_000, 0).unwrap().state, Haunted);
    // Motion at 14_400 restarts the grace period.
    assert_eq!(m.update(14_400, 60).unwrap(), snap(Haunted, 9900, true));
    assert_eq!(m.update(24_399, 0).unwrap().state, Haunted);
    assert_eq!(m.update(24_400, 0).unwrap().state, Recovering);
}

#[test]
fn between_thresholds_motion_does_not_extend_the_grace() {
    // Active at 5000, latch cleared at 5001 (last active stays 5000).
    let mut m = haunted();
    m.update(5000, 60).unwrap();
    m.update(5001, 0).unwrap();
    // A reading of 30 after the latch cleared is not active: no extension.
    assert_eq!(m.update(14_999, 30).unwrap(), snap(Haunted, 10_499, false));
    assert_eq!(m.update(15_000, 30).unwrap().state, Recovering);
}

#[test]
fn recovery_reentry_requires_a_new_debounce() {
    // haunted() last active at 4500: stillness ends at exactly 14_500.
    let mut m = haunted();
    assert_eq!(m.update(14_499, 0).unwrap().state, Haunted);
    assert_eq!(m.update(14_499, 0).unwrap().state, Haunted);
    assert_eq!(m.update(14_500, 0).unwrap(), snap(Recovering, 0, false));
    assert_eq!(
        m.update(14_600, 60).unwrap(),
        snap(PresenceDetected, 0, true)
    );
    assert_eq!(
        m.update(15_099, 60).unwrap(),
        snap(PresenceDetected, 499, true)
    );
    assert_eq!(m.update(15_100, 60).unwrap(), snap(Transforming, 0, true));
}

#[test]
fn recovering_ignores_inactive_readings_and_active_wins_at_the_boundary() {
    let mut m = haunted();
    m.update(24_500, 0).unwrap();
    assert_eq!(m.state(), Recovering);
    assert_eq!(m.update(25_000, 30).unwrap(), snap(Recovering, 500, false));
    // Activity at the recovery deadline goes to Presence, not Idle.
    assert_eq!(
        m.update(27_500, 60).unwrap(),
        snap(PresenceDetected, 0, true)
    );
}

#[test]
fn only_one_transition_per_update_even_after_a_huge_jump() {
    let mut m = machine();
    m.update(0, 60).unwrap();
    // A million ms passed, yet only Presence -> Transforming happens.
    assert_eq!(
        m.update(1_000_000, 60).unwrap(),
        snap(Transforming, 0, true)
    );
    assert_eq!(m.update(2_000_000, 60).unwrap(), snap(Haunted, 0, true));
    // Stillness since 2_000_000 is far past the grace period: Recovering only.
    assert_eq!(m.update(3_000_000, 0).unwrap(), snap(Recovering, 0, false));
    assert_eq!(m.update(4_000_000, 0).unwrap(), snap(Idle, 0, false));

    let mut h = haunted();
    assert_eq!(h.update(1_000_000, 0).unwrap(), snap(Recovering, 0, false));
    assert_eq!(h.update(2_000_000, 0).unwrap(), snap(Idle, 0, false));
}

#[test]
fn u64_max_is_supported_without_overflow() {
    let mut m = machine();
    assert_eq!(
        m.update(u64::MAX, 60).unwrap(),
        snap(PresenceDetected, 0, true)
    );
    assert_eq!(
        m.update(u64::MAX, 60).unwrap(),
        snap(PresenceDetected, 0, true)
    );
    assert_eq!(m.update(u64::MAX, 0).unwrap(), snap(Idle, 0, false));
    assert_eq!(m.update(u64::MAX, 0).unwrap(), snap(Idle, 0, false));

    let mut jump = haunted();
    assert_eq!(
        jump.update(u64::MAX, 0).unwrap(),
        snap(Recovering, 0, false)
    );
    assert_eq!(
        jump.update(u64::MAX, 0).unwrap(),
        snap(Recovering, 0, false)
    );
    assert_eq!(
        jump.update(u64::MAX, 60).unwrap(),
        snap(PresenceDetected, 0, true)
    );

    let mut start = machine();
    start.update(0, 60).unwrap();
    assert_eq!(
        start.update(u64::MAX, 60).unwrap(),
        snap(Transforming, 0, true)
    );
    assert_eq!(
        start.update(u64::MAX - 1, 60),
        Err(HauntError::TimeRegression)
    );
}

#[test]
fn first_update_establishes_zero_elapsed_at_any_time() {
    let mut m = machine();
    assert_eq!(m.update(1000, 0).unwrap(), snap(Idle, 0, false));
    assert_eq!(m.update(1500, 0).unwrap(), snap(Idle, 500, false));
    let mut late = machine();
    assert_eq!(
        late.update(5_000_000, 70).unwrap(),
        snap(PresenceDetected, 0, true)
    );
    assert_eq!(late.update(5_000_499, 70).unwrap().state, PresenceDetected);
    assert_eq!(late.update(5_000_500, 70).unwrap().state, Transforming);
}

#[test]
fn equal_timestamps_are_accepted_and_regressions_rejected() {
    let mut m = machine();
    m.update(100, 60).unwrap();
    assert_eq!(m.update(100, 60).unwrap(), snap(PresenceDetected, 0, true));
    assert_eq!(m.update(99, 60), Err(HauntError::TimeRegression));
    assert_eq!(m.update(100, 60).unwrap(), snap(PresenceDetected, 0, true));
}

#[test]
fn invalid_activity_is_rejected_first_and_1000_is_valid() {
    let mut m = machine();
    m.update(100, 0).unwrap();
    assert_eq!(m.update(200, 1001), Err(HauntError::ActivityOutOfRange));
    // Both problems at once: the activity error is reported.
    assert_eq!(m.update(50, 1001), Err(HauntError::ActivityOutOfRange));
    assert_eq!(m.update(50, 0), Err(HauntError::TimeRegression));
    assert_eq!(
        m.update(300, 1000).unwrap(),
        snap(PresenceDetected, 0, true)
    );
    assert_eq!(
        machine().update(0, u16::MAX),
        Err(HauntError::ActivityOutOfRange)
    );
}

#[test]
fn invalid_calls_leave_every_future_output_unchanged() {
    let mut a = machine();
    let mut b = machine();
    let script = [
        (0, 60),
        (200, 60),
        (500, 60),
        (3000, 0),
        (4600, 0),
        (9000, 70),
    ];
    for (i, &(t, act)) in script.iter().enumerate() {
        if i == 3 {
            // Bad calls that would change state if they mutated anything.
            assert!(a.update(10_000_000, 5000).is_err());
            assert!(a.update(1, 60).is_err());
            assert!(a.update(0, 0).is_err());
        }
        assert_eq!(a.update(t, act), b.update(t, act));
    }
    // A bad call on a fresh machine does not start its clock either.
    let mut fresh = machine();
    assert!(fresh.update(9_999_999, 1001).is_err());
    assert_eq!(fresh.update(5, 0).unwrap(), snap(Idle, 0, false));
}

#[test]
fn reset_clears_clock_latch_and_state_but_keeps_config() {
    let config = HauntConfig {
        debounce_ms: 100,
        ..HauntConfig::default()
    };
    let mut m = HauntMachine::new(config).unwrap();
    m.update(1000, 60).unwrap();
    m.update(1100, 60).unwrap();
    assert_eq!(m.state(), Transforming);
    m.reset();
    assert_eq!(m.state(), Idle);
    // Earlier timestamps are accepted; first update is elapsed zero.
    assert_eq!(m.update(0, 30).unwrap(), snap(Idle, 0, false));
    // The latch was cleared: 30 did not arm it. The custom debounce survives.
    assert_eq!(m.update(10, 60).unwrap(), snap(PresenceDetected, 0, true));
    assert_eq!(m.update(109, 60).unwrap().state, PresenceDetected);
    assert_eq!(m.update(110, 60).unwrap().state, Transforming);
    // Reset on a fresh machine and repeated resets are harmless.
    m.reset();
    m.reset();
    assert_eq!(m.update(u64::MAX, 0).unwrap(), snap(Idle, 0, false));
}

#[test]
fn reset_after_haunted_forgets_the_last_active_time() {
    let mut m = haunted();
    m.reset();
    m.update(0, 60).unwrap();
    m.update(500, 60).unwrap();
    // Stillness is measured from activity after reset, not from before it.
    assert_eq!(m.update(9_999 + 500, 0).unwrap().state, Haunted);
}

#[test]
fn configuration_validation_boundaries() {
    let ok = HauntConfig::default();
    assert!(HauntMachine::new(ok).is_ok());
    let bad =
        |c: HauntConfig| assert_eq!(HauntMachine::new(c).unwrap_err(), HauntError::InvalidConfig);
    let good = |c: HauntConfig| assert!(HauntMachine::new(c).is_ok(), "{c:?}");

    bad(HauntConfig {
        enter_activity: 0,
        leave_activity: 0,
        ..ok
    });
    bad(HauntConfig {
        enter_activity: 1000,
        ..ok
    });
    bad(HauntConfig {
        enter_activity: u16::MAX,
        ..ok
    });
    bad(HauntConfig {
        leave_activity: 50,
        ..ok
    });
    bad(HauntConfig {
        leave_activity: 51,
        ..ok
    });
    good(HauntConfig {
        enter_activity: 1,
        leave_activity: 0,
        ..ok
    });
    good(HauntConfig {
        enter_activity: 999,
        leave_activity: 998,
        ..ok
    });

    for field in 0..4 {
        let with = |v: u64| {
            let mut c = ok;
            match field {
                0 => c.debounce_ms = v,
                1 => c.transform_ms = v,
                2 => c.stillness_ms = v,
                _ => c.recovery_ms = v,
            }
            c
        };
        bad(with(0));
        bad(with(120_000));
        bad(with(u64::MAX));
        good(with(1));
        good(with(119_999));
    }
}

#[test]
fn error_text_and_traits() {
    for error in [
        HauntError::InvalidConfig,
        HauntError::ActivityOutOfRange,
        HauntError::TimeRegression,
    ] {
        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert!(!boxed.to_string().is_empty());
    }
    let s = snap(Idle, 0, false);
    assert_eq!(s, s);
}

// ---- Bounded seeded generated sequences (xorshift64, no external crate) ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

fn legal_edge(from: HauntState, to: HauntState) -> bool {
    matches!(
        (from, to),
        (Idle, PresenceDetected)
            | (PresenceDetected, Idle)
            | (PresenceDetected, Transforming)
            | (Transforming, Haunted)
            | (Transforming, Recovering)
            | (Haunted, Recovering)
            | (Recovering, PresenceDetected)
            | (Recovering, Idle)
    )
}

fn step(rng: &mut Rng) -> (u64, u16) {
    let dt = match rng.next() % 6 {
        0 => 0,
        1 => rng.next() % 50,
        2 => rng.next() % 700,
        3 => rng.next() % 5000,
        4 => rng.next() % 20_000,
        _ => rng.next() % 5_000_000,
    };
    let activity = match rng.next() % 5 {
        0 => 0,
        1 => 20 + (rng.next() % 3) as u16,
        2 => 49 + (rng.next() % 3) as u16,
        3 => (rng.next() % 1001) as u16,
        _ => 1000,
    };
    (dt, activity)
}

#[test]
fn generated_sequences_are_deterministic_legal_and_bounded() {
    let mut transitions_seen = std::collections::HashSet::new();
    let mut states_seen = std::collections::HashSet::new();
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (mut a, mut b) = (machine(), machine());
        let (mut now, mut prev): (u64, Option<(HauntSnapshot, u64, bool)>) =
            (rng.next() % 1000, None);
        let mut latch = false;
        for _ in 0..400 {
            let (dt, activity) = step(&mut rng);
            now = now.saturating_add(dt);
            let sa = a.update(now, activity).unwrap();
            let sb = b.update(now, activity).unwrap();
            assert_eq!(sa, sb, "seed {seed}: machines diverged");
            assert_eq!(a.state(), sa.state);
            // Independent model of the latch.
            if activity >= 50 {
                latch = true;
            } else if activity <= 20 {
                latch = false;
            }
            assert_eq!(sa.active, latch, "seed {seed}");
            if let Some((before, before_time, _)) = prev {
                if sa.state == before.state {
                    assert_eq!(
                        sa.state_elapsed_ms,
                        before.state_elapsed_ms + (now - before_time)
                    );
                } else {
                    assert!(
                        legal_edge(before.state, sa.state),
                        "seed {seed}: {before:?} -> {sa:?}"
                    );
                    assert_eq!(sa.state_elapsed_ms, 0);
                    transitions_seen.insert((before.state as u8, sa.state as u8));
                }
            } else {
                assert_eq!(sa.state_elapsed_ms, 0);
            }
            states_seen.insert(sa.state as u8);
            // Invalid calls never change what follows.
            if rng.next().is_multiple_of(7) {
                assert!(a.update(now, 1001 + (rng.next() % 60_000) as u16).is_err());
                if now > 0 {
                    assert!(a.update(now - 1, 0).is_err());
                }
            }
            prev = Some((sa, now, latch));
        }
    }
    // The generator must actually reach every state and every legal edge, or
    // the invariants above would be vacuous.
    assert_eq!(states_seen.len(), 5);
    assert_eq!(transitions_seen.len(), 8, "{transitions_seen:?}");
}
