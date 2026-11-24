use spook_core::{render_skull_rgba, OverlayRect, SkullOptions};
use spook_web::{MirrorEngine, MODE_MIRROR};

const W: u32 = 32;
const H: u32 = 24;
const BONE: [u32; 3] = [229, 223, 209];
/// Source-space head (unmirrored): x, y, width, height.
type Head = (i32, i32, u32, u32);
const HEAD: Head = (3, 2, 10, 12);

/// Asymmetric, alternating frames: the whole image changes every 100 ms, so
/// the shared motion estimate stays high. Strength reaches 1500 after 6500 ms
/// and 6000 at 11000 ms.
fn frame(time: u64) -> Vec<u8> {
    let flip = (time / 100) % 2 == 1;
    let mut data = Vec::new();
    for y in 0..H {
        for x in 0..W {
            let px = [x * 8, y * 10, (x * y) % 251, 200 + x % 50];
            for (index, value) in px.into_iter().enumerate() {
                let value = if flip && index < 3 {
                    255 - value
                } else {
                    value
                };
                data.push(value as u8);
            }
        }
    }
    data
}

fn automatic(skull: bool) -> MirrorEngine {
    let mut engine = MirrorEngine::new(W, H).unwrap();
    engine.configure_automatic(true, false).unwrap();
    if skull {
        engine.configure_skull(true).unwrap();
    }
    engine
}

fn observe(engine: &mut MirrorEngine, head: Head, confidence: u16, time: u64) {
    let (x, y, w, h) = head;
    engine
        .observe_head(x, y, w, h, confidence, time as f64)
        .unwrap();
}

/// A skull engine and a plain twin fed the same frames at the same times.
struct Pair {
    skull: MirrorEngine,
    plain: MirrorEngine,
}

impl Pair {
    fn new() -> Self {
        Self {
            skull: automatic(true),
            plain: automatic(false),
        }
    }

    /// Optionally observes a head at `time`, then processes both engines.
    fn run(&mut self, time: u64, head: Option<(Head, u16)>) -> (Vec<u8>, Vec<u8>) {
        match head {
            Some((bounds, confidence)) => observe(&mut self.skull, bounds, confidence, time),
            None => self.skull.clear_head(),
        }
        let input = frame(time);
        let out = self
            .skull
            .process_automatic_rgba(&input, time as f64)
            .unwrap();
        let base = self
            .plain
            .process_automatic_rgba(&input, time as f64)
            .unwrap();
        (out, base)
    }

    /// Runs 0..until in 100 ms steps without a head, then returns.
    fn warm(&mut self, until: u64) {
        for time in (0..until).step_by(100) {
            let (out, base) = self.run(time, None);
            assert_eq!(out, base);
        }
        // Strength is the milliseconds since the 5000 ms clear opening, so
        // the frame at 11000 ms is the first at full strength.
        assert_eq!(self.skull.haunt_strength(), until as u16 - 5_100);
    }
}

fn expected_opacity(elapsed: u64, strength: u64) -> u8 {
    let ramp = match elapsed {
        0..=1199 => elapsed,
        1200..=1999 => 1200,
        2000..=3199 => 3200 - elapsed,
        _ => 0,
    };
    ((96 * ramp * strength.min(6000) + 3_600_000) / 7_200_000) as u8
}

fn overlay(base: &[u8], head: Head, opacity: u8) -> Vec<u8> {
    let (x, y, w, h) = head;
    let mut out = vec![0; base.len()];
    let options = SkullOptions {
        bounds: OverlayRect {
            x: W as i32 - x - w as i32,
            y,
            width: w,
            height: h,
        },
        opacity,
    };
    render_skull_rgba(base, &mut out, W as usize, H as usize, options).unwrap();
    out
}

fn pixel(frame: &[u8], x: usize, y: usize) -> &[u8] {
    let at = (y * W as usize + x) * 4;
    &frame[at..at + 4]
}

fn changed(a: &[u8], b: &[u8]) -> Vec<(usize, usize)> {
    (0..H as usize)
        .flat_map(|y| (0..W as usize).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(a, x, y) != pixel(b, x, y))
        .collect()
}

#[test]
fn skull_matches_core_renderer_at_literal_mirrored_pixels() {
    let mut pair = Pair::new();
    pair.warm(11_000);
    for time in (11_000..=12_100).step_by(100) {
        pair.run(time, Some((HEAD, 1000)));
    }
    let (out, base) = pair.run(12_200, Some((HEAD, 1000)));
    assert_eq!(pair.skull.skull_opacity(), 96);
    // Source x=3, width 10 in a 32-wide frame mirrors to x = 32 - 3 - 10 = 19.
    assert_eq!(out, overlay(&base, HEAD, 96));
    assert_ne!(out, base);
    for (x, y) in changed(&out, &base) {
        assert!((19..29).contains(&x) && (2..14).contains(&y), "{x},{y}");
    }
    // Local (5, 2) lies inside the cranium: output pixel (24, 4).
    let before = pixel(&base, 24, 4);
    let after = pixel(&out, 24, 4);
    for channel in 0..3 {
        // alpha = (255 * 96 + 127) / 255 = 96 for a full mask.
        let expected = (u32::from(before[channel]) * 159 + BONE[channel] * 96 + 127) / 255;
        assert_eq!(u32::from(after[channel]), expected);
        assert_ne!(
            u32::from(after[channel]),
            BONE[channel],
            "face stays visible"
        );
    }
    assert_eq!(after[3], 255);
    assert_ne!(before, after);
    // The same box in the unmirrored place is a different image.
    let mut wrong = vec![0; base.len()];
    let unmirrored = SkullOptions {
        bounds: OverlayRect {
            x: 3,
            y: 2,
            width: 10,
            height: 12,
        },
        opacity: 96,
    };
    render_skull_rgba(&base, &mut wrong, W as usize, H as usize, unmirrored).unwrap();
    assert_ne!(out, wrong);
}

#[test]
fn off_center_bounds_land_at_the_mirrored_location() {
    for head in [(0, 0, 6, 6), (26, 18, 6, 6), (20, 5, 7, 9), (0, 18, 32, 6)] {
        let mut pair = Pair::new();
        pair.warm(11_000);
        for time in (11_000..=12_100).step_by(100) {
            pair.run(time, Some((head, 1000)));
        }
        let (out, base) = pair.run(12_200, Some((head, 1000)));
        assert_eq!(out, overlay(&base, head, 96), "{head:?}");
        let mirrored_x = (W as i32 - head.0 - head.2 as i32) as usize;
        for (x, y) in changed(&out, &base) {
            assert!(x >= mirrored_x && x < mirrored_x + head.2 as usize);
            assert!(y >= head.1 as usize && y < head.1 as usize + head.3 as usize);
        }
        assert!(!changed(&out, &base).is_empty());
    }
}

#[test]
fn fade_boundaries_follow_the_documented_timeline() {
    let mut pair = Pair::new();
    pair.warm(11_000);
    let start = 11_000;
    for time in (start..=start + 3_200).step_by(100) {
        let (out, base) = pair.run(time, Some((HEAD, 1000)));
        let elapsed = time - start;
        let strength = u64::from(pair.skull.haunt_strength());
        let expected = expected_opacity(elapsed, strength);
        assert_eq!(pair.skull.skull_opacity(), expected, "t={elapsed}");
        assert_eq!(out == base, expected == 0, "t={elapsed}");
        if expected > 0 {
            assert_eq!(out, overlay(&base, HEAD, expected));
        }
    }
    // Cooldown: a fresh head every frame produces no skull until start + 3200 + 8000.
    for time in (14_300..22_200).step_by(100) {
        let (out, base) = pair.run(time, Some((HEAD, 1000)));
        assert_eq!(pair.skull.skull_opacity(), 0, "t={time}");
        assert_eq!(out, base);
    }
    let (out, base) = pair.run(22_200, Some((HEAD, 1000)));
    assert_eq!((pair.skull.skull_opacity(), out == base), (0, true));
    pair.run(22_300, Some((HEAD, 1000)));
    assert!(pair.skull.skull_opacity() > 0);
}

#[test]
fn confidence_799_is_hidden_and_800_is_visible() {
    for (confidence, visible) in [(799, false), (800, true), (1000, true), (0, false)] {
        let mut pair = Pair::new();
        pair.warm(11_000);
        let mut last = (Vec::new(), Vec::new());
        for time in (11_000..=12_200).step_by(100) {
            last = pair.run(time, Some((HEAD, confidence)));
        }
        assert_eq!(last.0 != last.1, visible, "confidence {confidence}");
        assert_eq!(pair.skull.skull_opacity() > 0, visible);
    }
}

#[test]
fn age_500_is_visible_and_501_is_hidden_then_cools_down() {
    let mut pair = Pair::new();
    pair.warm(11_000);
    for time in (11_000..=12_100).step_by(100) {
        pair.run(time, Some((HEAD, 1000)));
    }
    // Last observation was at 12100. Frames without new observations:
    assert!(pair.skull.skull_opacity() > 0);
    // Age 500 at 12600 still draws (event at 1600 ms, hold).
    pair.plain
        .process_automatic_rgba(&frame(12_500), 12_500.0)
        .unwrap();
    pair.skull
        .process_automatic_rgba(&frame(12_500), 12_500.0)
        .unwrap();
    let input = frame(12_600);
    let out = pair.skull.process_automatic_rgba(&input, 12_600.0).unwrap();
    let base = pair.plain.process_automatic_rgba(&input, 12_600.0).unwrap();
    assert!(pair.skull.skull_opacity() > 0 && out != base);
    // Age 501 at 12601 hides the skull at once and cancels the event.
    let input = frame(12_601);
    let out = pair.skull.process_automatic_rgba(&input, 12_601.0).unwrap();
    let base = pair.plain.process_automatic_rgba(&input, 12_601.0).unwrap();
    assert_eq!((pair.skull.skull_opacity(), out == base), (0, true));
    // Cooldown starts at 12601 and lasts through 20600.
    for time in [12_700, 15_000, 20_600] {
        let (out, base) = pair.run(time, Some((HEAD, 1000)));
        assert_eq!(
            (pair.skull.skull_opacity(), out == base),
            (0, true),
            "{time}"
        );
    }
    pair.run(20_601, Some((HEAD, 1000)));
    assert_eq!(pair.skull.skull_opacity(), 0);
    pair.run(21_000, Some((HEAD, 1000)));
    assert!(pair.skull.skull_opacity() > 0);
}

#[test]
fn future_and_regressing_observations_are_suppressed_or_rejected() {
    let mut pair = Pair::new();
    pair.warm(11_000);
    for time in (11_000..=12_100).step_by(100) {
        pair.run(time, Some((HEAD, 1000)));
    }
    // Observation stamped after the frame: no skull, event cancelled.
    observe(&mut pair.skull, HEAD, 1000, 12_300);
    let input = frame(12_200);
    let out = pair.skull.process_automatic_rgba(&input, 12_200.0).unwrap();
    let base = pair.plain.process_automatic_rgba(&input, 12_200.0).unwrap();
    assert_eq!((pair.skull.skull_opacity(), out == base), (0, true));
    // Regression is rejected; equal time is accepted.
    assert!(pair
        .skull
        .observe_head(3, 2, 10, 12, 1000, 12_299.0)
        .is_err());
    pair.skull
        .observe_head(3, 2, 10, 12, 1000, 12_300.0)
        .unwrap();
    // A reset allows an earlier timestamp again.
    pair.skull.reset_temporal();
    pair.skull.observe_head(3, 2, 10, 12, 1000, 5.0).unwrap();
}

#[test]
fn invalid_input_leaves_a_twin_identical_for_all_future_results() {
    let mut a = automatic(true);
    let mut b = automatic(true);
    let run_invalid = |engine: &mut MirrorEngine, time: u64| {
        let last = time as f64;
        let (w, h) = (W as i32, H as i32);
        assert!(engine.observe_head(-1, 0, 4, 4, 900, last).is_err());
        assert!(engine.observe_head(0, -1, 4, 4, 900, last).is_err());
        assert!(engine.observe_head(0, 0, 0, 4, 900, last).is_err());
        assert!(engine.observe_head(0, 0, 4, 0, 900, last).is_err());
        assert!(engine.observe_head(w - 3, 0, 4, 4, 900, last).is_err());
        assert!(engine.observe_head(0, h - 3, 4, 4, 900, last).is_err());
        assert!(engine
            .observe_head(i32::MAX, 0, u32::MAX, 4, 900, last)
            .is_err());
        assert!(engine.observe_head(0, 0, 4, 4, 1001, last).is_err());
        assert!(engine.observe_head(0, 0, 4, 4, 900, f64::NAN).is_err());
        assert!(engine.observe_head(0, 0, 4, 4, 900, f64::INFINITY).is_err());
        assert!(engine.observe_head(0, 0, 4, 4, 900, last + 0.5).is_err());
        assert!(engine.observe_head(0, 0, 4, 4, 900, -1.0).is_err());
        assert!(engine
            .observe_head(0, 0, 4, 4, 900, 9_007_199_254_740_992.0)
            .is_err());
        if time > 11_000 {
            // Regression is only an error after an accepted observation.
            assert!(engine.observe_head(0, 0, 4, 4, 900, last - 1.0).is_err());
        }
        assert!(engine.process_automatic_rgba(&[0; 8], last).is_err());
        assert!(engine
            .process_automatic_rgba(&frame(time), f64::NAN)
            .is_err());
        assert!(engine
            .process_automatic_rgba(&frame(time), last - 101.0)
            .is_err());
    };
    for time in (0..=24_000).step_by(100) {
        if time >= 11_000 {
            observe(&mut a, HEAD, 1000, time);
            observe(&mut b, HEAD, 1000, time);
        }
        if time >= 5_000 {
            run_invalid(&mut a, time);
        }
        let input = frame(time);
        let out_a = a.process_automatic_rgba(&input, time as f64).unwrap();
        let out_b = b.process_automatic_rgba(&input, time as f64).unwrap();
        assert_eq!(out_a, out_b, "t={time}");
        assert_eq!(a.skull_opacity(), b.skull_opacity());
        assert_eq!(a.haunt_strength(), b.haunt_strength());
        if time == 12_200 {
            assert_eq!(a.skull_opacity(), 96);
        }
    }
}

#[test]
fn off_by_default_and_manual_methods_never_draw_a_skull() {
    let mut pair = Pair::new();
    pair.warm(11_000);
    // Skull never configured: valid observations are accepted and ignored.
    let mut off = automatic(false);
    let mut plain = automatic(false);
    for time in (0..=13_000).step_by(100) {
        observe(&mut off, HEAD, 1000, time);
        let input = frame(time);
        assert_eq!(
            off.process_automatic_rgba(&input, time as f64).unwrap(),
            plain.process_automatic_rgba(&input, time as f64).unwrap()
        );
        assert_eq!(off.skull_opacity(), 0);
    }
    let mut manual = MirrorEngine::new(W, H).unwrap();
    let mut reference = MirrorEngine::new(W, H).unwrap();
    manual.configure_skull(true).unwrap();
    observe(&mut manual, HEAD, 1000, 0);
    let input = frame(0);
    assert_eq!(
        manual.process_rgba(&input, MODE_MIRROR, 0).unwrap(),
        reference.process_rgba(&input, MODE_MIRROR, 0).unwrap()
    );
    assert_eq!(
        manual.process_timed_rgba(&input, 1, 100, 10.0).unwrap(),
        reference.process_timed_rgba(&input, 1, 100, 10.0).unwrap()
    );
    assert_eq!(manual.skull_opacity(), 0);
}

#[test]
fn reduced_motion_reset_and_head_loss_suppress_every_skull_pixel() {
    // Reduced motion from the start never reaches eligible strength.
    let mut reduced = MirrorEngine::new(W, H).unwrap();
    reduced.configure_automatic(true, true).unwrap();
    reduced.configure_skull(true).unwrap();
    let mut clear = MirrorEngine::new(W, H).unwrap();
    for time in (0..=14_000).step_by(100) {
        observe(&mut reduced, HEAD, 1000, time);
        let input = frame(time);
        let out = reduced.process_automatic_rgba(&input, time as f64).unwrap();
        assert_eq!(out, clear.process_rgba(&input, MODE_MIRROR, 0).unwrap());
        assert_eq!(reduced.skull_opacity(), 0);
    }

    for action in 0..4 {
        let mut pair = Pair::new();
        pair.warm(11_000);
        for time in (11_000..=12_200).step_by(100) {
            pair.run(time, Some((HEAD, 1000)));
        }
        assert_eq!(pair.skull.skull_opacity(), 96);
        let input = frame(12_300);
        let mirror = MirrorEngine::new(W, H)
            .unwrap()
            .process_rgba(&input, MODE_MIRROR, 0)
            .unwrap();
        match action {
            0 => {
                pair.skull.clear_head();
                assert_eq!(pair.skull.skull_opacity(), 0);
                let (out, base) = pair.run(12_300, None);
                assert_eq!(out, base);
            }
            2 => {
                pair.skull.configure_skull(false).unwrap();
                assert_eq!(pair.skull.skull_opacity(), 0);
                let (out, base) = pair.run(12_300, Some((HEAD, 1000)));
                assert_eq!(out, base);
            }
            _ => {
                if action == 1 {
                    pair.skull.reset_temporal();
                } else {
                    pair.skull.configure_automatic(true, true).unwrap();
                }
                assert_eq!(pair.skull.skull_opacity(), 0, "action {action}");
                // A fresh observation does not revive the old event.
                observe(&mut pair.skull, HEAD, 1000, 12_300);
                let out = pair.skull.process_automatic_rgba(&input, 12_300.0).unwrap();
                assert_eq!(out, mirror);
            }
        }
        assert_eq!(pair.skull.skull_opacity(), 0, "action {action}");
    }
}

#[test]
fn toggling_the_option_keeps_warm_history_and_identical_settings_continue() {
    let mut pair = Pair::new();
    pair.warm(11_000);
    for time in (11_000..=12_200).step_by(100) {
        pair.run(time, Some((HEAD, 1000)));
    }
    assert_eq!(pair.skull.skull_opacity(), 96);
    // Identical setting: the event continues with its observation.
    pair.skull.configure_skull(true).unwrap();
    let (out, base) = pair.run(12_300, Some((HEAD, 1000)));
    assert_eq!(pair.skull.skull_opacity(), 96);
    assert_eq!(out, overlay(&base, HEAD, 96));
    // Turning it off clears skull state only: the opening envelope and warm
    // temporal output still equal the plain twin, and nothing is drawn.
    pair.skull.configure_skull(false).unwrap();
    assert_eq!(pair.skull.skull_opacity(), 0);
    for time in (12_400..=13_000).step_by(100) {
        let (out, base) = pair.run(time, Some((HEAD, 1000)));
        assert_eq!(out, base);
        assert_eq!(pair.skull.haunt_strength(), 6000);
    }
    // Turning it back on clears the cooldown and the stale observation.
    pair.skull.configure_skull(true).unwrap();
    let (out, base) = pair.run(13_100, None);
    assert_eq!(out, base);
    let (out, base) = pair.run(13_200, Some((HEAD, 1000)));
    assert_eq!(out, base, "an event starts at zero opacity");
    pair.run(13_800, Some((HEAD, 1000)));
    assert!(pair.skull.skull_opacity() > 0);
}

#[test]
fn losing_the_head_hides_at_once_and_a_huge_jump_replays_nothing() {
    let mut pair = Pair::new();
    pair.warm(11_000);
    for time in (11_000..=12_200).step_by(100) {
        pair.run(time, Some((HEAD, 1000)));
    }
    let (out, base) = pair.run(12_300, None);
    assert_eq!((pair.skull.skull_opacity(), out == base), (0, true));
    // Cancellation cools down from 12300 until 20300.
    let (out, base) = pair.run(20_200, Some((HEAD, 1000)));
    assert_eq!(out, base);
    let far = 9_007_199_254_730_000u64;
    let mut runs = 0;
    let mut inside = false;
    for step in 0..60 {
        let time = far + step * 100;
        let (_, _) = pair.run(time, Some((HEAD, 1000)));
        let visible = pair.skull.skull_opacity() > 0;
        runs += usize::from(visible && !inside);
        inside = visible;
        if step == 0 {
            assert_eq!(pair.skull.skull_opacity(), 0);
        }
    }
    assert!(runs <= 1, "{runs}");
}
