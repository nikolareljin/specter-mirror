use spook_core::{render_skull_rgba, OverlayRect, SkullOptions};
use spook_web::{MirrorEngine, MODE_MIRROR};

const W: u32 = 32;
const H: u32 = 24;
/// Source-space head (unmirrored): x, y, width, height.
type Head = (i32, i32, u32, u32);
const HEAD: Head = (3, 2, 10, 12);
/// Unseeded session, head observed on every frame, skull enabled. Fixed
/// progression and director seed 0: the director selects LAG|ECHO at 6500 ms
/// and SKULL alone at 15900 ms (a neutral selection frame). The first positive
/// SKULL snapshot, and so the first apparition, is at 16000 ms; the event ends
/// at 21200 ms. The next SKULL event is selected at 34400 ms (first positive
/// snapshot 34500 ms), after the 8000 ms layer cooldown.
const FIRST: u64 = 16_000;
const SECOND: u64 = 34_500;

/// Asymmetric, alternating frames: the whole image changes every 100 ms, so
/// the shared motion estimate stays high.
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
        seed: Option<u32>,
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
                seed,
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

        /// A new session: same seed, no history, no motion sample.
        pub fn reset(&mut self) {
            let (progression, director) = Self::session(self.seed);
            self.progression = progression;
            self.director = director;
            self.history.reset();
            self.previous.fill(0);
            self.sampled = None;
            self.activity = 0;
            self.level = None;
            self.snapshot = DirectorSnapshot::default();
        }

        pub fn set_reduced(&mut self, reduced: bool) {
            self.reduced = reduced;
            self.reset();
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

/// Literal skull opacity for `elapsed` ms into an apparition (0..=96).
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

/// A skull engine and the independent reference fed the same frames. The rig
/// restates the head-availability rule (enabled, confidence >= 800, observed
/// <= now, age <= 500) and tracks the one-frame "no head" that `clear_head`
/// and a real skull toggle impose on the director.
struct Rig {
    engine: MirrorEngine,
    reference: Reference,
    skull_on: bool,
    head: Head,
    seen: Option<(u16, u64)>,
    no_head_once: bool,
}

impl Rig {
    fn new() -> Self {
        Self {
            engine: automatic(true),
            reference: Reference::new(W as usize, H as usize, None, false),
            skull_on: true,
            head: HEAD,
            seen: None,
            no_head_once: false,
        }
    }

    fn usable(&self, now: u64) -> bool {
        self.skull_on
            && !self.no_head_once
            && self
                .seen
                .is_some_and(|(c, at)| c >= 800 && at <= now && now - at <= 500)
    }

    fn observe(&mut self, head: Head, confidence: u16, observed: u64) {
        observe(&mut self.engine, head, confidence, observed);
        self.head = head;
        self.seen = Some((confidence, observed));
    }

    fn clear(&mut self) {
        self.engine.clear_head();
        self.seen = None;
        self.no_head_once = true;
        assert_eq!(self.engine.skull_opacity(), 0);
    }

    fn toggle(&mut self, on: bool) {
        self.engine.configure_skull(on).unwrap();
        self.skull_on = on;
        self.seen = None;
        self.no_head_once = true;
        assert_eq!(self.engine.skull_opacity(), 0);
    }

    /// Processes one frame in both pipelines, returning `(out, base)`.
    fn process(&mut self, time: u64) -> (Vec<u8>, Vec<u8>) {
        let input = frame(time);
        let out = self
            .engine
            .process_automatic_rgba(&input, time as f64)
            .unwrap();
        let usable = self.usable(time);
        let base = self.reference.step(&input, time, usable);
        self.no_head_once = false;
        (out, base)
    }

    /// Observes a fresh head (age 0), processes, and checks the output
    /// against the reference base plus the literal apparition opacity when an
    /// apparition began at `start`. Returns the opacity.
    fn tick(&mut self, time: u64, start: Option<u64>) -> u8 {
        self.observe(self.head, 1000, time);
        self.check(time, start)
    }

    fn check(&mut self, time: u64, start: Option<u64>) -> u8 {
        let (out, base) = self.process(time);
        let snap = self.reference.snapshot;
        let expected = match start {
            Some(s) if snap.layers & 4 != 0 && time >= s && time - s < 3200 => {
                expected_opacity(time - s, u64::from(snap.strength))
            }
            _ => 0,
        };
        assert_eq!(self.engine.skull_opacity(), expected, "t={time}");
        if expected > 0 {
            assert_eq!(out, overlay(&base, self.head, expected), "t={time}");
        } else {
            assert_eq!(out, base, "t={time}");
        }
        assert_eq!(
            self.engine.haunt_strength(),
            self.reference.level.unwrap().strength
        );
        expected
    }

    /// Runs frames `from..=to` every 100 ms with a fresh head.
    fn run(&mut self, from: u64, to: u64, start: Option<u64>) {
        for time in (from..=to).step_by(100) {
            self.tick(time, start);
        }
    }
}

#[test]
fn skull_matches_core_renderer_at_literal_mirrored_pixels() {
    let mut rig = Rig::new();
    rig.run(0, 16_900, Some(FIRST));
    let (out, base) = {
        rig.observe(HEAD, 1000, 17_200);
        rig.process(17_200)
    };
    // Event state: first positive snapshot at 16000, now 1200 ms into it.
    assert_eq!(rig.reference.snapshot.layers, 4);
    assert_eq!(rig.reference.snapshot.strength, 6000);
    assert_eq!(rig.engine.skull_opacity(), 96);
    // Source x=3, width 10 in a 32-wide frame mirrors to x = 32 - 3 - 10 = 19.
    assert_eq!(out, overlay(&base, HEAD, 96));
    assert_ne!(out, base);
    for (x, y) in changed(&out, &base) {
        assert!((19..29).contains(&x) && (2..14).contains(&y), "{x},{y}");
    }
    // Local (5, 2) lies inside the cranium: output pixel (24, 4). Literal
    // values: (43 * 159 + 229 * 96 + 127) / 255 = 113, 118 and 108 for the
    // base pixel [43, 55, 47] and bone [229, 223, 209] at alpha 96.
    assert_eq!(pixel(&base, 24, 4), [43, 55, 47, 255]);
    assert_eq!(pixel(&out, 24, 4), [113, 118, 108, 255]);
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
    // (head, literal mirrored output x = 32 - x - width)
    for (head, mirrored_x) in [
        ((0, 0, 6, 6), 26),
        ((26, 18, 6, 6), 0),
        ((20, 5, 7, 9), 5),
        ((0, 18, 32, 6), 0),
    ] {
        let mut rig = Rig::new();
        rig.head = head;
        rig.run(0, 17_200, Some(FIRST));
        assert_eq!(rig.engine.skull_opacity(), 96);
        // Compare against a base the reference rebuilds for this frame.
        let (out, base) = {
            rig.observe(head, 1000, 17_300);
            rig.process(17_300)
        };
        let opacity = rig.engine.skull_opacity();
        assert_eq!(opacity, 96, "{head:?}");
        assert_eq!(out, overlay(&base, head, 96), "{head:?}");
        let diff = changed(&out, &base);
        assert!(!diff.is_empty());
        for (x, y) in diff {
            assert!(x >= mirrored_x && x < mirrored_x + head.2 as usize, "{x}");
            assert!(y >= head.1 as usize && y < head.1 as usize + head.3 as usize);
        }
    }
}

#[test]
fn fade_boundaries_one_apparition_per_event_and_director_cooldown() {
    let mut rig = Rig::new();
    rig.run(0, 15_900, Some(FIRST));
    // Elapsed 0, 1, 1199, 1200, 1999, 2000, 3199 and 3200 ms into the
    // apparition (strength 6000 except in the first second of the event).
    let mut seen = Vec::new();
    for (time, literal) in [
        (16_000, 0),
        (16_001, 0),
        (17_199, 96),
        (17_200, 96),
        (17_999, 96),
        (18_000, 96),
        (19_199, 0),
        (19_200, 0),
    ] {
        let opacity = rig.tick(time, Some(FIRST));
        assert_eq!(opacity, literal, "t={time}");
        seen.push(opacity);
    }
    assert_eq!(rig.reference.snapshot.layers, 4);
    assert_eq!(seen.iter().filter(|&&o| o > 0).count(), 4);
    // The rest of the long SKULL event (ends at 21200 ms): still selected
    // with a fresh head every frame, but the consumed apparition never returns.
    let mut selected = 0;
    for time in (19_300..=21_100).step_by(100) {
        assert_eq!(rig.tick(time, Some(FIRST)), 0, "t={time}");
        selected += usize::from(rig.reference.snapshot.layers & 4 != 0);
    }
    assert!(selected >= 17, "SKULL stayed selected: {selected}");
    // Cooldown and a different event: no skull until a second director event.
    let mut rising = Vec::new();
    let mut inside = false;
    for time in (21_200..=45_000).step_by(100) {
        let opacity = rig.tick(time, Some(SECOND));
        if opacity > 0 && !inside {
            rising.push(time);
        }
        inside = opacity > 0;
        if time < SECOND {
            assert_eq!(opacity, 0, "t={time}");
        }
    }
    // 34600 is more than 8000 ms after the first event ended (21200).
    assert_eq!(rising, [34_600]);
}

#[test]
fn confidence_799_is_hidden_and_800_is_visible() {
    for (confidence, visible) in [(799, false), (800, true), (1000, true), (0, false)] {
        let mut rig = Rig::new();
        for time in (0..=17_200).step_by(100) {
            rig.observe(HEAD, confidence, time);
            let (out, base) = rig.process(time);
            if time == 17_200 {
                assert_eq!(rig.engine.skull_opacity() > 0, visible, "{confidence}");
                assert_eq!(out != base, visible, "{confidence}");
            } else if time < FIRST {
                assert_eq!(out, base);
            }
        }
        // Low confidence never reaches the director, so the candidate set
        // (and with it the whole selection sequence) differs from SKULL-able.
        let layers = rig.reference.snapshot.layers;
        assert_eq!(layers & 4 != 0, visible, "{confidence}");
    }
}

#[test]
fn age_500_is_visible_and_501_is_hidden_then_the_event_stays_cancelled() {
    let mut rig = Rig::new();
    rig.run(0, 17_000, Some(FIRST));
    // 1000 ms into the apparition: 96 * 1000 / 1200 = 80.
    assert_eq!(rig.engine.skull_opacity(), 80);
    // Last observation was at 17000. Age 500 still draws, age 501 does not.
    assert_eq!(rig.check(17_500, Some(FIRST)), 96);
    assert!(rig.reference.snapshot.layers & 4 != 0);
    assert_eq!(rig.check(17_501, Some(FIRST)), 0);
    assert_eq!(rig.reference.snapshot.layers, 0, "director cancelled");
    // The head returns inside what would have been the apparition window and
    // the director cooldown (8000 ms from 17501) keeps the event consumed.
    for time in (17_600..=25_500).step_by(100) {
        assert_eq!(rig.tick(time, Some(FIRST)), 0, "t={time}");
        assert_eq!(rig.reference.snapshot.layers & 4, 0, "t={time}");
    }
}

#[test]
fn future_and_regressing_observations_are_suppressed_or_rejected() {
    let mut rig = Rig::new();
    rig.run(0, 17_100, Some(FIRST));
    // 1100 ms into the apparition: 96 * 1100 / 1200 = 88.
    assert_eq!(rig.engine.skull_opacity(), 88);
    // Observation stamped after the frame: no skull, event cancelled.
    rig.observe(HEAD, 1000, 17_300);
    assert_eq!(rig.check(17_200, Some(FIRST)), 0);
    assert_eq!(rig.reference.snapshot.layers, 0);
    // Regression is rejected; equal time is accepted.
    assert!(rig
        .engine
        .observe_head(3, 2, 10, 12, 1000, 17_299.0)
        .is_err());
    rig.observe(HEAD, 1000, 17_300);
    // The now-valid observation cannot restart the cancelled event.
    for time in (17_300..=25_000).step_by(100) {
        rig.observe(HEAD, 1000, time);
        assert_eq!(rig.check(time, Some(FIRST)), 0);
    }
    // A reset allows an earlier timestamp again.
    rig.engine.reset_temporal();
    rig.engine.observe_head(3, 2, 10, 12, 1000, 5.0).unwrap();
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
        if time > 0 {
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
    let mut peak = 0;
    for time in (0..=40_000).step_by(100) {
        observe(&mut a, HEAD, 1000, time);
        observe(&mut b, HEAD, 1000, time);
        if time >= 500 {
            run_invalid(&mut a, time);
        }
        let input = frame(time);
        let out_a = a.process_automatic_rgba(&input, time as f64).unwrap();
        let out_b = b.process_automatic_rgba(&input, time as f64).unwrap();
        assert_eq!(out_a, out_b, "t={time}");
        assert_eq!(a.skull_opacity(), b.skull_opacity());
        assert_eq!(a.haunt_strength(), b.haunt_strength());
        if time == 17_200 {
            assert_eq!(a.skull_opacity(), 96);
        }
        peak = peak.max(a.skull_opacity());
    }
    assert_eq!(peak, 96);
}

#[test]
fn off_by_default_and_manual_methods_never_draw_a_skull() {
    // Skull never configured: valid observations are accepted and ignored.
    let mut off = automatic(false);
    let mut plain = automatic(false);
    let mut reference = Reference::new(W as usize, H as usize, None, false);
    for time in (0..=24_000).step_by(100) {
        observe(&mut off, HEAD, 1000, time);
        let input = frame(time);
        let out = off.process_automatic_rgba(&input, time as f64).unwrap();
        assert_eq!(
            out,
            plain.process_automatic_rgba(&input, time as f64).unwrap()
        );
        // A disabled skull is never available, so the director never picks it.
        assert_eq!(out, reference.step(&input, time, false));
        assert_eq!(reference.snapshot.layers & 4, 0);
        assert_eq!(off.skull_opacity(), 0);
    }
    let mut manual = MirrorEngine::new(W, H).unwrap();
    let mut clear = MirrorEngine::new(W, H).unwrap();
    manual.configure_skull(true).unwrap();
    observe(&mut manual, HEAD, 1000, 0);
    let input = frame(0);
    assert_eq!(
        manual.process_rgba(&input, MODE_MIRROR, 0).unwrap(),
        clear.process_rgba(&input, MODE_MIRROR, 0).unwrap()
    );
    assert_eq!(
        manual.process_timed_rgba(&input, 1, 100, 10.0).unwrap(),
        clear.process_timed_rgba(&input, 1, 100, 10.0).unwrap()
    );
    assert_eq!(manual.skull_opacity(), 0);
}

#[test]
fn reduced_motion_reset_and_head_loss_suppress_every_skull_pixel() {
    // Reduced motion from the start: neutral output, no skull ever.
    let mut reduced = MirrorEngine::new(W, H).unwrap();
    reduced.configure_automatic(true, true).unwrap();
    reduced.configure_skull(true).unwrap();
    let mut clear = MirrorEngine::new(W, H).unwrap();
    for time in (0..=40_000).step_by(100) {
        observe(&mut reduced, HEAD, 1000, time);
        let input = frame(time);
        let out = reduced.process_automatic_rgba(&input, time as f64).unwrap();
        assert_eq!(out, clear.process_rgba(&input, MODE_MIRROR, 0).unwrap());
        assert_eq!(reduced.skull_opacity(), 0);
    }

    for action in 0..4 {
        let mut rig = Rig::new();
        rig.run(0, 17_200, Some(FIRST));
        assert_eq!(rig.engine.skull_opacity(), 96);
        let input = frame(17_300);
        let mirror = MirrorEngine::new(W, H)
            .unwrap()
            .process_rgba(&input, MODE_MIRROR, 0)
            .unwrap();
        match action {
            0 => {
                rig.clear();
                // Head returns before the next frame: still cancelled.
                rig.observe(HEAD, 1000, 17_300);
                assert_eq!(rig.check(17_300, Some(FIRST)), 0);
                assert_eq!(rig.reference.snapshot.layers, 0);
                rig.run(17_400, 26_000, Some(FIRST));
            }
            1 => {
                rig.toggle(false);
                assert_eq!(rig.check(17_300, Some(FIRST)), 0);
            }
            _ => {
                if action == 2 {
                    rig.engine.reset_temporal();
                    rig.reference.reset();
                } else {
                    rig.engine.configure_automatic(true, true).unwrap();
                    rig.reference.set_reduced(true);
                }
                rig.seen = None;
                assert_eq!(rig.engine.skull_opacity(), 0, "action {action}");
                // A fresh observation does not revive the old event.
                rig.observe(HEAD, 1000, 17_300);
                let (out, base) = rig.process(17_300);
                assert_eq!(out, base);
                if action == 3 {
                    assert_eq!(out, mirror);
                }
            }
        }
        assert_eq!(rig.engine.skull_opacity(), 0, "action {action}");
    }
}

#[test]
fn toggling_preserves_history_and_progression_without_a_fresh_event() {
    let mut rig = Rig::new();
    rig.run(0, 17_200, Some(FIRST));
    assert_eq!(rig.engine.skull_opacity(), 96);
    // Identical setting: the event continues with its observation.
    rig.engine.configure_skull(true).unwrap();
    assert_eq!(rig.tick(17_300, Some(FIRST)), 96);
    // Off: skull state clears at once; the director sees no head and cancels
    // the event, but warm history and the progression continue unchanged.
    rig.toggle(false);
    for time in (17_400..=24_000).step_by(100) {
        assert_eq!(rig.check(time, Some(FIRST)), 0, "t={time}");
        assert_eq!(rig.engine.haunt_strength(), 6000);
        assert_eq!(rig.reference.snapshot.layers & 4, 0);
    }
    // On again: an unconsumed cooldown applies, so no event is manufactured.
    // The layer cooldown runs from the cancellation frame (17400) for 8000 ms.
    rig.toggle(true);
    let mut first_positive = None;
    for time in (24_100..=45_000).step_by(100) {
        rig.observe(HEAD, 1000, time);
        let (out, base) = rig.process(time);
        let positive = rig.reference.snapshot.layers & 4 != 0;
        if positive && first_positive.is_none() {
            first_positive = Some(time);
        }
        let expected = match first_positive {
            Some(start) if time - start < 3200 => {
                expected_opacity(time - start, u64::from(rig.reference.snapshot.strength))
            }
            _ => 0,
        };
        assert_eq!(rig.engine.skull_opacity(), expected, "t={time}");
        assert_eq!(out == base, expected == 0, "t={time}");
    }
    let start = first_positive.unwrap();
    assert!(start >= 17_400 + 8000, "{start}");
}

#[test]
fn a_huge_jump_and_the_maximum_timestamp_replay_nothing() {
    let mut rig = Rig::new();
    rig.run(0, 17_200, Some(FIRST));
    rig.clear();
    rig.observe(HEAD, 1000, 17_300);
    assert_eq!(rig.check(17_300, Some(FIRST)), 0);
    let far = 9_007_199_254_730_000u64;
    let mut runs = 0;
    let mut inside = false;
    let mut start = None;
    for step in 0..60 {
        let time = far + step * 100;
        rig.observe(HEAD, 1000, time);
        let (out, base) = rig.process(time);
        let snap = rig.reference.snapshot;
        if snap.layers & 4 != 0 && start.is_none() {
            start = Some(time);
        }
        let expected = match start {
            Some(s) if time - s < 3200 && snap.layers & 4 != 0 => {
                expected_opacity(time - s, u64::from(snap.strength))
            }
            _ => 0,
        };
        assert_eq!(rig.engine.skull_opacity(), expected, "step {step}");
        assert_eq!(out == base, expected == 0);
        let visible = expected > 0;
        runs += usize::from(visible && !inside);
        inside = visible;
        if step == 0 {
            assert_eq!(expected, 0);
        }
    }
    assert!(runs <= 1, "{runs}");
    // The largest accepted browser timestamp, then the same instant again.
    let max = 9_007_199_254_740_991u64;
    rig.observe(HEAD, 1000, max);
    let (out, base) = rig.process(max);
    assert_eq!(rig.engine.skull_opacity(), 0);
    assert_eq!(out, base);
    rig.observe(HEAD, 1000, max);
    let (out, base) = rig.process(max);
    assert_eq!(out, base);
    assert!(rig
        .engine
        .process_automatic_rgba(&frame(0), 9_007_199_254_740_992.0)
        .is_err());
}

#[test]
fn repeated_timestamps_never_rearm_or_advance_the_apparition() {
    let mut rig = Rig::new();
    rig.run(0, 15_900, Some(FIRST));
    // The first positive snapshot and the 1200 ms and 3200 ms frames, each
    // processed three times at the same instant.
    for (time, literal) in [(16_000, 0), (17_200, 96), (19_200, 0)] {
        for _ in 0..3 {
            assert_eq!(rig.tick(time, Some(FIRST)), literal, "t={time}");
        }
        rig.run(time + 100, time + 600, Some(FIRST));
    }
    // 19800: the same long SKULL event is still selected and stays consumed.
    assert!(rig.reference.snapshot.layers & 4 != 0);
    assert_eq!(rig.engine.skull_opacity(), 0);
}
