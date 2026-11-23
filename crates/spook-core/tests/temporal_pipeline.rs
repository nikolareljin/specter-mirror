//! Exact temporal results. Frame k is a flat background plus one marker
//! pixel that moves asymmetrically; expected pixels are literals computed
//! independently of the implementation (see the formula in `blend` comments).

use spook_core::{TemporalError, TemporalHistory, TemporalOptions};

const SIZES: [(usize, usize); 2] = [(160, 120), (320, 240)];

fn opts(delay_ms: u16, trail_strength: u8) -> TemporalOptions {
    TemporalOptions {
        delay_ms,
        trail_strength,
    }
}

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

/// Source frame k; alpha 7 so forced opaque output is observable.
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

fn px(out: &[u8], w: usize, (x, y): (usize, usize)) -> [u8; 4] {
    let i = (y * w + x) * 4;
    [out[i], out[i + 1], out[i + 2], out[i + 3]]
}

/// Feeds frames 0..=last at t = 100 * k with the same options and returns
/// the output of the final call. Intermediate outputs are discarded, so a
/// ring that stored blended output would corrupt the final result.
fn run(w: usize, h: usize, last: usize, options: TemporalOptions) -> Vec<u8> {
    let mut history = TemporalHistory::new(w, h).unwrap();
    let mut out = vec![0; w * h * 4];
    for k in 0..=last {
        history
            .process(&frame(w, h, k), &mut out, 100 * k as u64, options)
            .unwrap();
    }
    out
}

/// Frame k with alpha forced to 255, as the engine must output it.
fn opaque_frame(w: usize, h: usize, k: usize) -> Vec<u8> {
    let mut v = frame(w, h, k);
    v.iter_mut().skip(3).step_by(4).for_each(|a| *a = 255);
    v
}

fn opaque(c: [u8; 3]) -> [u8; 4] {
    [c[0], c[1], c[2], 255]
}

#[test]
fn zero_options_return_current_rgb_with_opaque_alpha() {
    for (w, h) in SIZES {
        let out = run(w, h, 5, opts(0, 0));
        assert_eq!(px(&out, w, marker_pos(5)), opaque(marker(5)));
        assert_eq!(px(&out, w, (0, 0)), [110, 175, 55, 255]);
        assert_eq!(px(&out, w, marker_pos(3)), [110, 175, 55, 255]);
        assert!(out.iter().skip(3).step_by(4).all(|&a| a == 255));
    }
}

#[test]
fn delay_alone_shows_the_selected_past_frame() {
    for (w, h) in SIZES {
        // t = 500, delay 250: newest sample at or before 250 is k = 2 (t = 200).
        let out = run(w, h, 5, opts(250, 0));
        assert_eq!(px(&out, w, (0, 0)), [50, 220, 25, 255]);
        assert_eq!(px(&out, w, (7, 5)), [253, 102, 202, 255]);
        // Frame 5's marker is absent: only the delayed frame is visible.
        assert_eq!(px(&out, w, (16, 11)), [50, 220, 25, 255]);
    }
}

#[test]
fn trails_alone_blend_the_frame_200ms_earlier() {
    for (w, h) in SIZES {
        // t = 500: base k = 5, trail target 300 -> k = 3. Strength 128:
        // (base * 127 + trail * 128 + 127) / 255, e.g. R (110*127+70*128+127)/255 = 90.
        let out = run(w, h, 5, opts(0, 128));
        assert_eq!(px(&out, w, (0, 0)), [90, 190, 45, 255]);
        assert_eq!(px(&out, w, (16, 11)), [160, 155, 120, 255]);
        assert_eq!(px(&out, w, (10, 7)), [181, 139, 129, 255]);
    }
}

#[test]
fn delay_and_trails_combine() {
    for (w, h) in SIZES {
        // t = 500, delay 150: base target 350 -> k = 3; trail target 150 -> k = 1.
        let out = run(w, h, 5, opts(150, 128));
        assert_eq!(px(&out, w, (0, 0)), [50, 220, 25, 255]);
        assert_eq!(px(&out, w, (10, 7)), [141, 169, 109, 255]);
        assert_eq!(px(&out, w, (4, 3)), [162, 153, 118, 255]);
    }
}

#[test]
fn full_strength_trail_is_exactly_the_selected_past_frame() {
    for (w, h) in SIZES {
        // delay 100 -> trail target 200 -> k = 2, regardless of the k = 4 base.
        let out = run(w, h, 5, opts(100, 255));
        let expected = opaque_frame(w, h, 2);
        assert_eq!(out, expected);
    }
}

#[test]
fn warm_up_uses_the_oldest_sample_and_not_zeroed_storage() {
    for (w, h) in SIZES {
        let mut history = TemporalHistory::new(w, h).unwrap();
        let mut out = vec![0; w * h * 4];
        history
            .process(&frame(w, h, 3), &mut out, 1_000, opts(600, 255))
            .unwrap();
        let expected = opaque_frame(w, h, 3);
        assert_eq!(out, expected);
        // Second call, 100 ms later: still younger than the delay.
        history
            .process(&frame(w, h, 4), &mut out, 1_100, opts(600, 0))
            .unwrap();
        assert_eq!(px(&out, w, (0, 0)), opaque(bg(3)));
    }
}

#[test]
fn sampling_boundary_is_100ms_and_equal_timestamps_do_not_store() {
    let (w, h) = (160, 120);
    let off = opts(0, 0);
    let delayed = opts(600, 0);
    let mut out = vec![0; w * h * 4];

    // Equal timestamp: accepted, not sampled, so the oldest is still frame 0.
    let mut equal = TemporalHistory::new(w, h).unwrap();
    equal
        .process(&frame(w, h, 0), &mut out, 10, delayed)
        .unwrap();
    equal
        .process(&frame(w, h, 1), &mut out, 10, delayed)
        .unwrap();
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(0)));

    // 99 ms after the stored sample: skipped. Target 99 would pick frame 1
    // if it had been stored.
    let mut early = TemporalHistory::new(w, h).unwrap();
    early.process(&frame(w, h, 0), &mut out, 0, off).unwrap();
    early.process(&frame(w, h, 1), &mut out, 99, off).unwrap();
    early
        .process(&frame(w, h, 2), &mut out, 699, delayed)
        .unwrap();
    assert_eq!(
        px(&out, w, (0, 0)),
        opaque(bg(0)),
        "99 ms sample must be skipped"
    );

    // Exactly 100 ms: sampled. Target 100 selects frame 1.
    let mut exact = TemporalHistory::new(w, h).unwrap();
    exact.process(&frame(w, h, 0), &mut out, 0, off).unwrap();
    exact.process(&frame(w, h, 1), &mut out, 100, off).unwrap();
    exact
        .process(&frame(w, h, 2), &mut out, 700, delayed)
        .unwrap();
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(1)));
}

#[test]
fn sparse_capture_stores_one_sample_not_the_missed_intervals() {
    let (w, h) = (160, 120);
    let mut history = TemporalHistory::new(w, h).unwrap();
    let mut out = vec![0; w * h * 4];
    history
        .process(&frame(w, h, 0), &mut out, 0, opts(0, 0))
        .unwrap();
    history
        .process(&frame(w, h, 1), &mut out, 1_000, opts(0, 0))
        .unwrap();
    // Full-strength trail 200 ms before t = 1_000 reaches frame 0, not a copy of 1.
    history
        .process(&frame(w, h, 2), &mut out, 1_100, opts(0, 255))
        .unwrap();
    // Target 900: frame 0 is the newest sample at or before it.
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(0)));
    // Frame 1 was stored once at t = 1_000: delay 100 from 1_100 selects it.
    history
        .process(&frame(w, h, 3), &mut out, 1_100, opts(100, 0))
        .unwrap();
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(1)));
}

#[test]
fn wraparound_keeps_the_newest_eight_in_chronological_order() {
    let (w, h) = (160, 120);
    let mut history = TemporalHistory::new(w, h).unwrap();
    let mut out = vec![0; w * h * 4];
    // 12 frames; ring keeps k = 4..=11 (t = 400..=1100).
    for k in 0..12 {
        history
            .process(&frame(w, h, k), &mut out, 100 * k as u64, opts(600, 0))
            .unwrap();
    }
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(5)));
    // The oldest retained frame answers anything older.
    let mut history = TemporalHistory::new(w, h).unwrap();
    for k in 0..12 {
        history
            .process(&frame(w, h, k), &mut out, 100 * k as u64, opts(600, 255))
            .unwrap();
    }
    // Trail target 1100 - 600 - 200 = 300, older than k = 4 -> k = 4.
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(4)));
    assert_eq!(px(&out, w, marker_pos(4)), opaque(marker(4)));
}

#[test]
fn storage_stays_bounded_and_wraps_at_640_by_480() {
    let (w, h) = (640, 480);
    let mut history = TemporalHistory::new(w, h).unwrap();
    assert_eq!(history.storage_bytes(), 3 * 640 * 480 * 4);
    assert!(history.storage_bytes() <= 4 * 1024 * 1024);
    let mut out = vec![0; w * h * 4];
    for k in 0..5 {
        history
            .process(&frame(w, h, k), &mut out, 100 * k as u64, opts(600, 0))
            .unwrap();
    }
    // Ring holds k = 2, 3, 4; nothing is old enough, so the oldest (k = 2).
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(2)));
    assert_eq!(px(&out, w, marker_pos(2)), opaque(marker(2)));
    assert_eq!(history.storage_bytes(), 3 * 640 * 480 * 4);
}

#[test]
fn reset_drops_every_old_frame_and_clock() {
    let (w, h) = (160, 120);
    let mut history = TemporalHistory::new(w, h).unwrap();
    let mut out = vec![0; w * h * 4];
    for k in 0..4 {
        history
            .process(
                &frame(w, h, k),
                &mut out,
                1_000 + 100 * k as u64,
                opts(0, 0),
            )
            .unwrap();
    }
    let before = history.storage_bytes();
    history.reset();
    assert_eq!(history.storage_bytes(), before);
    // Earlier timestamp is fine after reset; only the new source is visible.
    history
        .process(&frame(w, h, 5), &mut out, 0, opts(600, 255))
        .unwrap();
    let expected = opaque_frame(w, h, 5);
    assert_eq!(out, expected);
    history.reset();
    history
        .process(&frame(w, h, 6), &mut out, 0, opts(600, 128))
        .unwrap();
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(6)));
}

#[test]
fn u64_max_does_not_panic_and_selects_by_elapsed_time() {
    let (w, h) = (160, 120);
    let mut history = TemporalHistory::new(w, h).unwrap();
    let mut out = vec![0; w * h * 4];
    history
        .process(&frame(w, h, 0), &mut out, 0, opts(600, 0))
        .unwrap();
    history
        .process(&frame(w, h, 1), &mut out, u64::MAX, opts(600, 0))
        .unwrap();
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(0)));
    history
        .process(&frame(w, h, 2), &mut out, u64::MAX, opts(0, 255))
        .unwrap();
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(0)));
    assert_eq!(
        history.process(&frame(w, h, 3), &mut out, u64::MAX - 1, opts(0, 0)),
        Err(TemporalError::TimeRegression)
    );
    let mut first = TemporalHistory::new(w, h).unwrap();
    first
        .process(&frame(w, h, 4), &mut out, u64::MAX, opts(600, 255))
        .unwrap();
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(4)));
}

#[test]
fn output_suffix_is_preserved() {
    let (w, h) = (160, 120);
    let mut history = TemporalHistory::new(w, h).unwrap();
    let mut out = vec![0xAB; w * h * 4 + 13];
    history
        .process(&frame(w, h, 1), &mut out, 0, opts(0, 0))
        .unwrap();
    assert!(out[w * h * 4..].iter().all(|&b| b == 0xAB));
    assert_eq!(px(&out, w, (0, 0)), opaque(bg(1)));
}

#[test]
fn invalid_calls_preserve_output_and_all_state() {
    let (w, h) = (160, 120);
    let size = w * h * 4;
    let mut a = TemporalHistory::new(w, h).unwrap();
    let mut b = TemporalHistory::new(w, h).unwrap();
    let options = opts(250, 128);
    let mut out_a = vec![0; size];
    let mut out_b = vec![0; size];
    for k in 0..3 {
        a.process(&frame(w, h, k), &mut out_a, 100 * k as u64, options)
            .unwrap();
        b.process(&frame(w, h, k), &mut out_b, 100 * k as u64, options)
            .unwrap();
    }
    let sentinel = vec![0x5A; size + 3];
    let mut probe = sentinel.clone();
    let good = frame(w, h, 9);
    assert_eq!(
        a.process(&good[1..], &mut probe, 300, options),
        Err(TemporalError::InputLength)
    );
    assert_eq!(
        a.process(&good, &mut probe[..size - 1], 300, options),
        Err(TemporalError::OutputTooShort)
    );
    assert_eq!(
        a.process(&good, &mut probe, 300, opts(601, 128)),
        Err(TemporalError::DelayOutOfRange)
    );
    assert_eq!(
        a.process(&good, &mut probe, 199, options),
        Err(TemporalError::TimeRegression)
    );
    assert_eq!(probe, sentinel);
    // A twin that never saw the bad calls must stay in lockstep with `a`.
    for k in 3..8 {
        a.process(&frame(w, h, k), &mut out_a, 100 * k as u64, options)
            .unwrap();
        b.process(&frame(w, h, k), &mut out_b, 100 * k as u64, options)
            .unwrap();
        assert_eq!(out_a, out_b);
    }
}

#[test]
fn constructor_and_option_errors_are_stable() {
    assert_eq!(
        TemporalHistory::new(0, 1).unwrap_err(),
        TemporalError::EmptyDimension
    );
    assert_eq!(
        TemporalHistory::new(641, 480).unwrap_err(),
        TemporalError::DimensionsTooLarge
    );
    assert_eq!(
        TemporalHistory::new(usize::MAX, usize::MAX).unwrap_err(),
        TemporalError::DimensionsOverflow
    );
    assert_eq!(
        TemporalError::TimeRegression.to_string(),
        "timestamp is earlier than the previous call"
    );
    let boxed: Box<dyn std::error::Error> = Box::new(TemporalError::InputLength);
    assert!(!boxed.to_string().is_empty());
    let o = opts(600, 255);
    assert_eq!(o, o.clone());
}
