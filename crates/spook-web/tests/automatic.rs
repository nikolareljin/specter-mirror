use spook_web::{MirrorEngine, MODE_MIRROR};

fn automatic() -> MirrorEngine {
    let mut engine = MirrorEngine::new(2, 1).unwrap();
    engine.configure_automatic(true, false).unwrap();
    engine
}

fn frame(time: u64) -> [u8; 8] {
    let v = if (time / 100).is_multiple_of(2) {
        0
    } else {
        255
    };
    [v, v, v, 7, v, v, v, 9]
}

#[test]
fn opening_is_clear_then_only_director_selected_layers_reach_literal_pixels() {
    let mut engine = automatic();
    let mut clear = MirrorEngine::new(2, 1).unwrap();
    // Literal output pixel (red channel; both pixels and channels are equal)
    // at selected times. Unseeded session: fixed progression, director seed 0,
    // no head. Frames alternate black (even 100 ms index) and white.
    let literal = |time: u64| -> Option<u8> {
        match time {
            // Progression strength 1500 (palette weight 35), director neutral:
            // the selection frame shows the current white frame, not the
            // delayed black one the old progression-scaled path produced.
            6500 => Some(220),
            // First positive snapshot: LAG|ECHO with delay 11 ms and trail 2
            // read the white frame 100 ms earlier; palette weight 37.
            6600 => Some(218),
            // Event over, neutral again, full-strength palette on white.
            12_900 => Some(115),
            // ECHO only (delay 0): the current black frame stays black. The
            // old path delayed it 250 ms onto a white frame.
            21_000 => Some(0),
            // LAG only (37 ms) shows the previous black frame instead of white.
            29_300 => Some(0),
            _ => None,
        }
    };
    for time in (0..=30_000).step_by(100) {
        let input = frame(time);
        let output = engine.process_automatic_rgba(&input, time as f64).unwrap();
        if time <= 5000 {
            assert_eq!(output, clear.process_rgba(&input, MODE_MIRROR, 0).unwrap());
            assert_eq!(engine.haunt_strength(), 0);
        }
        if let Some(red) = literal(time) {
            assert_eq!(output, [red, red, red, 255, red, red, red, 255], "t={time}");
        }
    }
    assert_eq!(engine.haunt_strength(), 6000);
    assert_eq!(engine.motion_activity(), 1000);
    assert_eq!(engine.haunt_state(), 3);
}

#[test]
fn preferences_and_reset_do_not_reuse_an_old_source() {
    let mut engine = automatic();
    for time in (0..=11000).step_by(100) {
        engine
            .process_automatic_rgba(&frame(time), time as f64)
            .unwrap();
    }
    assert_eq!(engine.haunt_strength(), 6000);
    engine.configure_automatic(true, true).unwrap();
    for time in (0..=12000).step_by(100) {
        let expected = MirrorEngine::new(2, 1)
            .unwrap()
            .process_rgba(&frame(time), MODE_MIRROR, 0)
            .unwrap();
        assert_eq!(
            engine
                .process_automatic_rgba(&frame(time), time as f64)
                .unwrap(),
            expected
        );
        assert_eq!(engine.haunt_strength(), 0);
    }
    engine.reset_temporal();
    assert_eq!(engine.haunt_state(), 0);
    assert_eq!(engine.motion_activity(), 0);
    assert_eq!(
        engine.process_automatic_rgba(&[17; 8], 0.0).unwrap(),
        [17, 17, 17, 255, 17, 17, 17, 255]
    );
    engine.configure_automatic(false, false).unwrap();
    assert!(engine.process_automatic_rgba(&frame(0), 0.0).is_err());
}

#[test]
fn malformed_frames_and_times_leave_all_future_results_unchanged() {
    let (mut a, mut b) = (automatic(), automatic());
    for time in (0..=8000).step_by(100) {
        assert_eq!(
            a.process_automatic_rgba(&frame(time), time as f64).unwrap(),
            b.process_automatic_rgba(&frame(time), time as f64).unwrap()
        );
    }
    for time in [
        f64::NAN,
        f64::INFINITY,
        -1.0,
        1.5,
        7999.0,
        9_007_199_254_740_992.0,
    ] {
        assert!(a.process_automatic_rgba(&frame(0), time).is_err());
    }
    assert!(a.process_automatic_rgba(&[0; 7], 90000.0).is_err());
    a.configure_automatic(true, false).unwrap();
    for time in (8100..=11000).step_by(100) {
        assert_eq!(
            a.process_automatic_rgba(&frame(time), time as f64).unwrap(),
            b.process_automatic_rgba(&frame(time), time as f64).unwrap()
        );
        assert_eq!(a.haunt_strength(), b.haunt_strength());
    }
}

#[test]
fn quiet_frames_allow_recovery_to_clear() {
    let mut engine = automatic();
    for time in (0..=11000).step_by(100) {
        engine
            .process_automatic_rgba(&frame(time), time as f64)
            .unwrap();
    }
    for time in (11100..=24000).step_by(100) {
        engine.process_automatic_rgba(&[0; 8], time as f64).unwrap();
        if time == 21000 {
            assert_eq!((engine.haunt_state(), engine.haunt_strength()), (4, 6000));
        }
        if time == 22500 {
            assert_eq!(engine.haunt_strength(), 3000);
        }
    }
    assert_eq!((engine.haunt_state(), engine.haunt_strength()), (0, 0));
}
