use spook_core::{HauntError, HauntProgression, HauntState};

fn rising() -> HauntProgression {
    let mut progression = HauntProgression::new().unwrap();
    progression.update(0, 10, false).unwrap();
    progression.update(500, 10, false).unwrap();
    progression
}

#[test]
fn opening_is_clear_and_effects_ramp_at_literal_times() {
    let mut p = rising();
    assert_eq!(p.update(4999, 10, false).unwrap().strength, 0);
    assert_eq!(p.update(5000, 10, false).unwrap().strength, 0);
    assert_eq!(p.update(6500, 10, false).unwrap().strength, 1500);
    assert_eq!(p.update(8000, 10, false).unwrap().strength, 3000);
    assert_eq!(p.update(11000, 10, false).unwrap().strength, 6000);
    assert_eq!(p.update(12000, 10, false).unwrap().strength, 6000);
}

#[test]
fn ramp_uses_elapsed_time_at_different_update_rates() {
    let mut coarse = rising();
    let mut fine = rising();
    coarse.update(5000, 10, false).unwrap();
    for time in (550..=5000).step_by(50) {
        fine.update(time, 10, false).unwrap();
    }
    let coarse_level = coarse.update(8000, 10, false).unwrap();
    let mut fine_level = fine.update(5000, 10, false).unwrap();
    for time in (5050..=8000).step_by(50) {
        fine_level = fine.update(time, 10, false).unwrap();
    }
    assert_eq!(coarse_level, fine_level);
    assert_eq!(coarse_level.strength, 3000);
}

#[test]
fn returning_motion_preserves_the_partial_recovery_level() {
    let mut p = rising();
    for time in [5000, 6500, 11000] {
        p.update(time, 10, false).unwrap();
    }
    p.update(21000, 0, false).unwrap();
    assert_eq!(p.update(22500, 0, false).unwrap().strength, 3000);
    assert_eq!(p.update(22600, 10, false).unwrap().strength, 2800);
    assert_eq!(p.update(23100, 10, false).unwrap().strength, 2800);
    assert_eq!(p.update(23600, 10, false).unwrap().strength, 3300);
}

#[test]
fn stillness_gets_grace_then_fades_instead_of_jumping() {
    let mut p = rising();
    for time in [5000, 6500, 11000] {
        p.update(time, 10, false).unwrap();
    }
    assert_eq!(p.update(20999, 0, false).unwrap().strength, 6000);
    let start = p.update(21000, 0, false).unwrap();
    assert_eq!(
        (start.state, start.strength),
        (HauntState::Recovering, 6000)
    );
    assert_eq!(p.update(22500, 0, false).unwrap().strength, 3000);
    let end = p.update(24000, 0, false).unwrap();
    assert_eq!((end.state, end.strength), (HauntState::Idle, 0));
}

#[test]
fn reduced_motion_and_reset_clear_strength_and_errors_preserve_state() {
    let mut a = rising();
    let mut b = rising();
    assert_eq!(
        a.update(100000, 1001, false),
        Err(HauntError::ActivityOutOfRange)
    );
    assert_eq!(a.update(499, 0, false), Err(HauntError::TimeRegression));
    for time in [5000, 7000, 11000] {
        assert_eq!(a.update(time, 10, false), b.update(time, 10, false));
    }
    assert_eq!(a.update(11001, 10, true).unwrap().strength, 0);
    assert!(a.update(11002, 10, false).unwrap().strength <= 1);
    a.reset();
    assert_eq!(a.update(0, 10, false).unwrap().strength, 0);
    assert_eq!(a.update(500, 10, false).unwrap().strength, 0);
}

#[test]
fn large_and_equal_timestamps_remain_bounded_and_idle_stays_clear() {
    let mut p = HauntProgression::new().unwrap();
    for time in [0, 10000, u64::MAX, u64::MAX] {
        let level = p.update(time, 0, false).unwrap();
        assert_eq!(level.strength, 0);
        assert_eq!(level.state, HauntState::Idle);
    }
    p.reset();
    assert_eq!(p.update(u64::MAX, 1000, false).unwrap().strength, 0);
}
