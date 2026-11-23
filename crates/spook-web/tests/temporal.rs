use spook_web::{MirrorEngine, MODE_MIRROR, MODE_PORTRAIT, MODE_SPECTRAL};

// Frame k: flat background plus one marker pixel that moves asymmetrically.
// Expected pixels are literals worked out by hand/independently, in source
// coordinates; the engine mirrors horizontally exactly once.

fn bg(k: usize) -> [u8; 3] {
    [
        (10 + 20 * k) as u8,
        (250 - 15 * k) as u8,
        (5 + 10 * k) as u8,
    ]
}

fn marker(k: usize) -> [u8; 3] {
    [(255 - k) as u8, (100 + k) as u8, (200 + k) as u8]
}

fn marker_pos(k: usize) -> (usize, usize) {
    (3 * k + 1, 2 * k + 1)
}

fn frame(w: usize, h: usize, k: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(w * h * 4);
    let (mx, my) = marker_pos(k);
    for y in 0..h {
        for x in 0..w {
            let c = if (x, y) == (mx, my) { marker(k) } else { bg(k) };
            v.extend_from_slice(&[c[0], c[1], c[2], 7]);
        }
    }
    v
}

/// Reads a pixel by its SOURCE x; the output is mirrored.
fn src_px(out: &[u8], w: usize, (x, y): (usize, usize)) -> [u8; 4] {
    let i = (y * w + (w - 1 - x)) * 4;
    [out[i], out[i + 1], out[i + 2], out[i + 3]]
}

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

const W: usize = 160;
const H: usize = 120;

fn engine(delay: u16, trail: u8) -> MirrorEngine {
    let mut e = MirrorEngine::new(W as u32, H as u32).unwrap();
    e.configure_temporal(delay, trail).unwrap();
    e
}

/// Feeds frames 0..=last at t = 100 * k; returns the final output.
fn run(e: &mut MirrorEngine, last: usize, mode: u8) -> Vec<u8> {
    let mut out = Vec::new();
    for k in 0..=last {
        out = e
            .process_timed_rgba(&frame(W, H, k), mode, 255, (100 * k) as f64)
            .unwrap();
    }
    out
}

#[test]
fn zero_options_match_process_rgba_byte_for_byte() {
    let mut timed = MirrorEngine::new(8, 6).unwrap();
    let mut plain = MirrorEngine::new(8, 6).unwrap();
    for (i, mode) in [MODE_MIRROR, MODE_SPECTRAL, MODE_PORTRAIT]
        .into_iter()
        .enumerate()
    {
        for k in 0..4 {
            let input = frame(8, 6, k);
            let a = timed
                .process_timed_rgba(&input, mode, (k * 70) as u8, (i * 1000 + k * 40) as f64)
                .unwrap();
            assert_eq!(a, plain.process_rgba(&input, mode, (k * 70) as u8).unwrap());
        }
    }
    // Enabled then disabled again: still identical, no stale frames.
    timed.configure_temporal(300, 200).unwrap();
    timed
        .process_timed_rgba(&frame(8, 6, 1), MODE_MIRROR, 0, 0.0)
        .unwrap();
    timed.configure_temporal(0, 0).unwrap();
    let input = frame(8, 6, 3);
    assert_eq!(
        timed
            .process_timed_rgba(&input, MODE_SPECTRAL, 200, 0.0)
            .unwrap(),
        plain.process_rgba(&input, MODE_SPECTRAL, 200).unwrap()
    );
}

#[test]
fn delay_trails_and_mirror_use_literal_pixels() {
    // t = 500 (k = 5). Delay 250 -> frame 2: marker (7,5), flat [50,220,25].
    let out = run(&mut engine(250, 0), 5, MODE_MIRROR);
    assert_eq!(src_px(&out, W, (7, 5)), [253, 102, 202, 255]);
    assert_eq!(src_px(&out, W, (16, 11)), [50, 220, 25, 255]);
    // Mirrored exactly once: the marker sits at W - 1 - 7 in the output.
    let i = (5 * W + (W - 1 - 7)) * 4;
    assert_eq!(&out[i..i + 4], &[253, 102, 202, 255]);

    // Trails only: base k = 5, trail k = 3, strength 128.
    let out = run(&mut engine(0, 128), 5, MODE_MIRROR);
    assert_eq!(src_px(&out, W, (0, 0)), [90, 190, 45, 255]);
    assert_eq!(src_px(&out, W, (16, 11)), [160, 155, 120, 255]);
    assert_eq!(src_px(&out, W, (10, 7)), [181, 139, 129, 255]);

    // Combined: delay 150 -> base k = 3, trail target 150 -> k = 1.
    let out = run(&mut engine(150, 128), 5, MODE_MIRROR);
    assert_eq!(src_px(&out, W, (0, 0)), [50, 220, 25, 255]);
    assert_eq!(src_px(&out, W, (10, 7)), [141, 169, 109, 255]);
    assert_eq!(src_px(&out, W, (4, 3)), [162, 153, 118, 255]);
}

#[test]
fn temporal_runs_before_edge_and_palette_processing() {
    // The result must equal the untouched pipeline applied to the selected
    // past frame (frame 2 at delay 250), for both processed modes.
    for mode in [MODE_SPECTRAL, MODE_PORTRAIT] {
        let timed = run(&mut engine(250, 0), 5, mode);
        let expected = MirrorEngine::new(W as u32, H as u32)
            .unwrap()
            .process_rgba(&frame(W, H, 2), mode, 255)
            .unwrap();
        assert_eq!(timed, expected, "mode {mode}");
    }
}

#[test]
fn process_rgba_is_stateless_for_temporal_history() {
    let mut a = engine(250, 128);
    let mut b = engine(250, 128);
    for k in 0..6 {
        let t = (100 * k) as f64;
        // Only `a` interleaves untimed calls with unrelated frames.
        a.process_rgba(&frame(W, H, 9 - k), MODE_MIRROR, 0).unwrap();
        let ra = a
            .process_timed_rgba(&frame(W, H, k), MODE_MIRROR, 0, t)
            .unwrap();
        let rb = b
            .process_timed_rgba(&frame(W, H, k), MODE_MIRROR, 0, t)
            .unwrap();
        assert_eq!(ra, rb);
    }
}

#[test]
fn elapsed_time_boundaries() {
    let mut e = engine(100, 0);
    let input = frame(W, H, 1);
    for ok in [0.0, -0.0, 1.0, 9_007_199_254_740_991.0] {
        let mut e = engine(100, 0);
        assert!(
            e.process_timed_rgba(&input, MODE_MIRROR, 0, ok).is_ok(),
            "{ok}"
        );
    }
    for bad in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.0,
        0.5,
        1.0e-9,
        9_007_199_254_740_992.0,
        1.0e300,
    ] {
        assert!(
            e.process_timed_rgba(&input, MODE_MIRROR, 0, bad).is_err(),
            "{bad}"
        );
    }
    // The failed calls did not start the clock: time 0 is still available.
    assert!(e.process_timed_rgba(&input, MODE_MIRROR, 0, 0.0).is_ok());
}

#[test]
fn regressing_time_is_rejected_but_equal_time_and_reset_are_allowed() {
    let mut e = engine(100, 0);
    let input = frame(W, H, 1);
    e.process_timed_rgba(&input, MODE_MIRROR, 0, 500.0).unwrap();
    assert!(e.process_timed_rgba(&input, MODE_MIRROR, 0, 499.0).is_err());
    assert!(e.process_timed_rgba(&input, MODE_MIRROR, 0, 500.0).is_ok());
    e.reset_temporal();
    assert!(e.process_timed_rgba(&input, MODE_MIRROR, 0, 0.0).is_ok());
    // Zero options also track time.
    let mut z = MirrorEngine::new(W as u32, H as u32).unwrap();
    z.process_timed_rgba(&input, MODE_MIRROR, 0, 10.0).unwrap();
    assert!(z.process_timed_rgba(&input, MODE_MIRROR, 0, 9.0).is_err());
}

#[test]
fn errors_do_not_advance_temporal_state() {
    let mut a = engine(250, 128);
    let mut b = engine(250, 128);
    for k in 0..3 {
        let t = (100 * k) as f64;
        a.process_timed_rgba(&frame(W, H, k), MODE_MIRROR, 0, t)
            .unwrap();
        b.process_timed_rgba(&frame(W, H, k), MODE_MIRROR, 0, t)
            .unwrap();
    }
    // Bad calls that would sample a distinct frame at t = 400 if they mutated.
    let poison = frame(W, H, 9);
    assert!(a.process_timed_rgba(&poison, 3, 0, 400.0).is_err());
    assert!(a
        .process_timed_rgba(&poison[4..], MODE_MIRROR, 0, 400.0)
        .is_err());
    assert!(a
        .process_timed_rgba(&poison, MODE_MIRROR, 0, f64::NAN)
        .is_err());
    assert!(a.process_timed_rgba(&poison, MODE_MIRROR, 0, 1.5).is_err());
    assert!(a
        .process_timed_rgba(&poison, MODE_MIRROR, 0, 150.0)
        .is_err());
    // Clock did not move to 400 or 150: 250 is still a valid next time.
    for k in 3..8 {
        let t = (100 * k) as f64;
        let ra = a
            .process_timed_rgba(&frame(W, H, k), MODE_MIRROR, 0, t)
            .unwrap();
        let rb = b
            .process_timed_rgba(&frame(W, H, k), MODE_MIRROR, 0, t)
            .unwrap();
        assert_eq!(ra, rb);
    }
}

#[test]
fn bad_mode_and_size_leave_the_next_result_unchanged() {
    let mut a = engine(0, 255);
    let mut b = engine(0, 255);
    for e in [&mut a, &mut b] {
        e.process_timed_rgba(&frame(W, H, 0), MODE_MIRROR, 0, 0.0)
            .unwrap();
    }
    assert!(a.process_timed_rgba(&frame(W, H, 7), 9, 0, 100.0).is_err());
    assert!(a
        .process_timed_rgba(&[0; 3], MODE_MIRROR, 0, 100.0)
        .is_err());
    let ra = a
        .process_timed_rgba(&frame(W, H, 2), MODE_MIRROR, 0, 100.0)
        .unwrap();
    let rb = b
        .process_timed_rgba(&frame(W, H, 2), MODE_MIRROR, 0, 100.0)
        .unwrap();
    assert_eq!(ra, rb);
    // Trail strength 255 at t = 100 selects the oldest (frame 0) sample.
    assert_eq!(src_px(&ra, W, (0, 0)), opaque(bg(0)));
}

#[test]
fn configuration_changes_reset_but_identical_settings_do_not() {
    let mut e = engine(250, 0);
    for k in 0..5 {
        e.process_timed_rgba(&frame(W, H, k), MODE_MIRROR, 0, (100 * k) as f64)
            .unwrap();
    }
    // Same values: history and clock survive; t = 500 -> target 250 -> frame 2.
    e.configure_temporal(250, 0).unwrap();
    let out = e
        .process_timed_rgba(&frame(W, H, 5), MODE_MIRROR, 0, 500.0)
        .unwrap();
    assert_eq!(src_px(&out, W, (0, 0)), opaque(bg(2)));
    // Rejected values change nothing.
    assert!(e.configure_temporal(601, 9).is_err());
    let out = e
        .process_timed_rgba(&frame(W, H, 6), MODE_MIRROR, 0, 600.0)
        .unwrap();
    assert_eq!(src_px(&out, W, (0, 0)), opaque(bg(3)));
    // A real change clears history and time: only the new source shows,
    // and an earlier timestamp is accepted.
    e.configure_temporal(300, 0).unwrap();
    let out = e
        .process_timed_rgba(&frame(W, H, 7), MODE_MIRROR, 0, 0.0)
        .unwrap();
    assert_eq!(src_px(&out, W, (0, 0)), opaque(bg(7)));
    // Disabling clears valid frames; re-enabling shows no stale content.
    e.configure_temporal(0, 0).unwrap();
    e.configure_temporal(600, 255).unwrap();
    let out = e
        .process_timed_rgba(&frame(W, H, 8), MODE_MIRROR, 0, 0.0)
        .unwrap();
    assert_eq!(src_px(&out, W, (0, 0)), opaque(bg(8)));
}

#[test]
fn reset_temporal_keeps_settings_and_forgets_the_source() {
    let mut e = engine(250, 0);
    for k in 0..4 {
        e.process_timed_rgba(&frame(W, H, k), MODE_MIRROR, 0, (1000 + 100 * k) as f64)
            .unwrap();
    }
    e.reset_temporal();
    let out = e
        .process_timed_rgba(&frame(W, H, 5), MODE_MIRROR, 0, 0.0)
        .unwrap();
    assert_eq!(src_px(&out, W, (0, 0)), opaque(bg(5)));
    // Delay setting persists: a later call at t = 300 selects t = 0 sample.
    let out = e
        .process_timed_rgba(&frame(W, H, 6), MODE_MIRROR, 0, 300.0)
        .unwrap();
    assert_eq!(src_px(&out, W, (0, 0)), opaque(bg(5)));
    // Reset on an engine that never enabled temporal is harmless.
    MirrorEngine::new(4, 4).unwrap().reset_temporal();
}

#[test]
fn full_size_engine_runs_with_bounded_history() {
    let (w, h) = (640usize, 480usize);
    let mut e = MirrorEngine::new(w as u32, h as u32).unwrap();
    e.configure_temporal(600, 255).unwrap();
    let mut out = Vec::new();
    for k in 0..5 {
        out = e
            .process_timed_rgba(&frame(w, h, k), MODE_MIRROR, 0, (100 * k) as f64)
            .unwrap();
    }
    // Three slots keep frames 2..=4; the oldest answers the full-strength trail.
    assert_eq!(src_px(&out, w, (0, 0)), opaque(bg(2)));
    assert_eq!(src_px(&out, w, marker_pos(2)), opaque(marker(2)));
}
