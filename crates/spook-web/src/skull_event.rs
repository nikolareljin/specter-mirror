//! Head observations and the timed skull event for the browser engine.
//!
//! The layer owns one fallibly allocated RGBA scratch frame and fixed
//! metadata. It never infers a head from motion: bounds come only from
//! `observe`, in unmirrored processing-frame pixels.
//!
//! # Placement
//!
//! An observation is usable at processing time `now` only when its confidence
//! is at least 800 (of 1000), `observed <= now`, and `now - observed <= 500`
//! ms. Order is checked before subtraction. Bounds are stored unmirrored and
//! transformed once, at render time: `output_x = frame_width - x - width`.
//!
//! # Event timeline
//!
//! Eligible frames need automatic haunting (the caller), shared strength of at
//! least 1500, usable placement and reduced motion off. The first eligible
//! frame starts an event at `t = 0`:
//!
//! ```text
//! 0..1200 ms      fade in   ramp = t
//! 1200..2000 ms   hold      ramp = 1200
//! 2000..3200 ms   fade out  ramp = 3200 - t
//! 3200 ms         ended     opacity 0
//! ```
//!
//! `opacity = (96 * ramp * min(strength, 6000) + 3_600_000) / 7_200_000`,
//! which is 96 * (ramp / 1200) * (strength / 6000) rounded half up in
//! integers. The maximum is 96 (the core renderer accepts up to 128).
//!
//! A natural end starts the 8000 ms cooldown at `start + 3200`. Losing
//! eligibility (placement, strength, reduced motion) cancels the event on that
//! frame and starts the cooldown at that frame's time. `clear_head` cancels
//! immediately and starts the cooldown at the next processed frame. The
//! cooldown is one deadline, so a large clock jump yields at most one new
//! event on the first eligible frame after it; missed events never replay.
use spook_core::{render_skull_rgba, OverlayRect, SkullOptions, HAUNT_FULL_STRENGTH};

pub(crate) const MIN_CONFIDENCE: u16 = 800;
pub(crate) const MAX_CONFIDENCE: u16 = 1000;
pub(crate) const MAX_AGE_MS: u64 = 500;
pub(crate) const MIN_STRENGTH: u16 = 1500;
pub(crate) const FADE_MS: u64 = 1200;
pub(crate) const HOLD_MS: u64 = 800;
pub(crate) const EVENT_MS: u64 = FADE_MS + HOLD_MS + FADE_MS;
pub(crate) const COOLDOWN_MS: u64 = 8000;
pub(crate) const MAX_OPACITY: u64 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Observation {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    confidence: u16,
    observed_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Active { start: u64 },
}

struct Step {
    phase: Phase,
    cooldown_until: Option<u64>,
    opacity: u8,
}

pub(crate) struct SkullLayer {
    enabled: bool,
    scratch: Vec<u8>,
    observation: Option<Observation>,
    last_observed_ms: Option<u64>,
    phase: Phase,
    cooldown_until: Option<u64>,
    cancel_pending: bool,
    opacity: u8,
}

/// Skull opacity for `elapsed` ms into an event at the given shared strength.
pub(crate) fn opacity_at(elapsed: u64, strength: u16) -> u8 {
    let ramp = if elapsed < FADE_MS {
        elapsed
    } else if elapsed < FADE_MS + HOLD_MS {
        FADE_MS
    } else {
        // Fade out; zero once the event has ended.
        EVENT_MS.saturating_sub(elapsed)
    };
    let strength = u64::from(strength).min(u64::from(HAUNT_FULL_STRENGTH));
    let denominator = FADE_MS * u64::from(HAUNT_FULL_STRENGTH);
    ((MAX_OPACITY * ramp * strength + denominator / 2) / denominator) as u8
}

impl SkullLayer {
    pub fn new() -> Self {
        Self {
            enabled: false,
            scratch: Vec::new(),
            observation: None,
            last_observed_ms: None,
            phase: Phase::Idle,
            cooldown_until: None,
            cancel_pending: false,
            opacity: 0,
        }
    }

    pub fn opacity(&self) -> u8 {
        self.opacity
    }

    /// Enables or disables the layer. The scratch frame (`pixels * 4` bytes)
    /// is allocated here, once. Allocation failure changes nothing. Identical
    /// settings keep all state; a real change clears observations and events.
    pub fn configure(&mut self, enabled: bool, pixels: usize) -> Result<(), String> {
        if enabled == self.enabled {
            return Ok(());
        }
        if enabled && self.scratch.is_empty() {
            let bytes = pixels
                .checked_mul(4)
                .ok_or("skull working-frame size overflows")?;
            let mut scratch = Vec::new();
            scratch
                .try_reserve_exact(bytes)
                .map_err(|_| "skull working-frame allocation failed")?;
            scratch.resize(bytes, 0);
            self.scratch = scratch;
        }
        self.enabled = enabled;
        self.reset();
        Ok(())
    }

    /// Clears observations, cooldown, event phase and opacity; keeps settings.
    pub fn reset(&mut self) {
        self.observation = None;
        self.last_observed_ms = None;
        self.phase = Phase::Idle;
        self.cooldown_until = None;
        self.cancel_pending = false;
        self.opacity = 0;
    }

    /// Validates and stores unmirrored head bounds. Errors change no state.
    /// A disabled layer validates but stores nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn observe(
        &mut self,
        frame_width: usize,
        frame_height: usize,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        confidence: u16,
        observed_ms: u64,
    ) -> Result<(), String> {
        if width == 0 || height == 0 {
            return Err("head bounds must not be empty".into());
        }
        if x < 0
            || y < 0
            || i64::from(x) + i64::from(width) > frame_width as i64
            || i64::from(y) + i64::from(height) > frame_height as i64
        {
            return Err("head bounds must lie inside the processing frame".into());
        }
        if confidence > MAX_CONFIDENCE {
            return Err("head confidence must be from 0 to 1000".into());
        }
        if self.last_observed_ms.is_some_and(|last| observed_ms < last) {
            return Err("head observation time must not decrease".into());
        }
        if !self.enabled {
            return Ok(());
        }
        self.observation = Some(Observation {
            x: x as u32,
            y: y as u32,
            width,
            height,
            confidence,
            observed_ms,
        });
        self.last_observed_ms = Some(observed_ms);
        Ok(())
    }

    /// Forgets the observation and cancels any event at once.
    pub fn clear_head(&mut self) {
        self.observation = None;
        if matches!(self.phase, Phase::Active { .. }) {
            self.phase = Phase::Idle;
            self.cancel_pending = true;
        }
        self.opacity = 0;
    }

    fn placement(&self, now: u64) -> Option<Observation> {
        let seen = self.observation?;
        if seen.confidence < MIN_CONFIDENCE || seen.observed_ms > now {
            return None;
        }
        (now - seen.observed_ms <= MAX_AGE_MS).then_some(seen)
    }

    fn step(&self, now: u64, strength: u16, reduced: bool) -> Step {
        let mut cooldown_until = self.cooldown_until;
        if self.cancel_pending {
            cooldown_until = Some(now + COOLDOWN_MS);
        }
        let eligible = !reduced && strength >= MIN_STRENGTH && self.placement(now).is_some();
        let mut phase = self.phase;
        if let Phase::Active { start } = phase {
            if now.saturating_sub(start) >= EVENT_MS {
                phase = Phase::Idle;
                cooldown_until = Some(start + EVENT_MS + COOLDOWN_MS);
            } else if !eligible {
                phase = Phase::Idle;
                cooldown_until = Some(now + COOLDOWN_MS);
            }
        }
        if phase == Phase::Idle && eligible && cooldown_until.is_none_or(|due| now >= due) {
            phase = Phase::Active { start: now };
        }
        let opacity = match phase {
            Phase::Active { start } => opacity_at(now.saturating_sub(start), strength),
            Phase::Idle => 0,
        };
        Step {
            phase,
            cooldown_until,
            opacity,
        }
    }

    /// Advances the event for one valid automatic frame and, when visible,
    /// draws the skull over `out` (the final mirrored RGBA). State is
    /// committed only after drawing succeeds.
    pub fn apply(
        &mut self,
        out: &mut [u8],
        width: usize,
        height: usize,
        now: u64,
        strength: u16,
        reduced: bool,
    ) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let step = self.step(now, strength, reduced);
        if step.opacity > 0 {
            let seen = self
                .placement(now)
                .ok_or("skull event has no usable placement")?;
            if self.scratch.len() != out.len() {
                return Err("skull working frame does not match the output".into());
            }
            self.scratch.copy_from_slice(out);
            // Transform the unmirrored bounds once for the mirrored output.
            let mirrored = (width as i64) - i64::from(seen.x) - i64::from(seen.width);
            let bounds = OverlayRect {
                x: i32::try_from(mirrored).map_err(|_| "skull bounds overflow")?,
                y: seen.y as i32,
                width: seen.width,
                height: seen.height,
            };
            let options = SkullOptions {
                bounds,
                opacity: step.opacity,
            };
            render_skull_rgba(&self.scratch, out, width, height, options)
                .map_err(|e| e.to_string())?;
        }
        self.phase = step.phase;
        self.cooldown_until = step.cooldown_until;
        self.cancel_pending = false;
        self.opacity = step.opacity;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 8;
    const H: usize = 8;

    fn layer() -> SkullLayer {
        let mut layer = SkullLayer::new();
        layer.configure(true, W * H).unwrap();
        layer
    }

    fn frame() -> Vec<u8> {
        vec![40; W * H * 4]
    }

    /// Observe at `now` (age 0) then process; returns the reported opacity.
    fn tick(layer: &mut SkullLayer, now: u64, strength: u16) -> u8 {
        layer.observe(W, H, 1, 1, 4, 5, 1000, now).unwrap();
        layer
            .apply(&mut frame(), W, H, now, strength, false)
            .unwrap();
        layer.opacity()
    }

    #[test]
    fn opacity_boundaries_use_documented_rounding() {
        for (elapsed, strength, expected) in [
            (0, 6000, 0),
            (1, 6000, 0),
            (13, 6000, 1),
            (600, 6000, 48),
            (1199, 6000, 96),
            (1200, 6000, 96),
            (1999, 6000, 96),
            (2000, 6000, 96),
            (2001, 6000, 96),
            (2600, 6000, 48),
            (3199, 6000, 0),
            (3200, 6000, 0),
            (9999, 6000, 0),
            (1200, 1500, 24),
            (600, 1500, 12),
            (1200, 3000, 48),
            (1200, 9000, 96),
            (1200, 1499, 24),
        ] {
            assert_eq!(
                opacity_at(elapsed, strength),
                expected,
                "{elapsed} {strength}"
            );
        }
        for elapsed in 0..=4000 {
            assert!(opacity_at(elapsed, u16::MAX) <= 96);
        }
    }

    #[test]
    fn timeline_cooldown_and_second_event() {
        let mut l = layer();
        assert_eq!(tick(&mut l, 1000, 6000), 0);
        assert_eq!(tick(&mut l, 1600, 6000), 48);
        assert_eq!(tick(&mut l, 2200, 6000), 96);
        assert_eq!(tick(&mut l, 2999, 6000), 96);
        assert_eq!(tick(&mut l, 3200, 6000), 80);
        assert_eq!(tick(&mut l, 3800, 6000), 32);
        // Event ends at 1000 + 3200; cooldown runs until 12200.
        assert_eq!(tick(&mut l, 4200, 6000), 0);
        assert_eq!(tick(&mut l, 12199, 6000), 0);
        assert_eq!(tick(&mut l, 12200, 6000), 0);
        assert_eq!(tick(&mut l, 12800, 6000), 48);
    }

    #[test]
    fn reduced_motion_and_low_strength_never_start_an_event() {
        let mut l = layer();
        for now in (0..4000).step_by(100) {
            l.observe(W, H, 1, 1, 4, 5, 1000, now).unwrap();
            l.apply(&mut frame(), W, H, now, 6000, true).unwrap();
            assert_eq!((l.opacity(), l.phase), (0, Phase::Idle));
            l.apply(&mut frame(), W, H, now, 1499, false).unwrap();
            assert_eq!((l.opacity(), l.phase), (0, Phase::Idle));
        }
        assert!(l.cooldown_until.is_none());
        l.apply(&mut frame(), W, H, 4000, 1500, false).unwrap();
        assert_eq!(l.phase, Phase::Active { start: 4000 });
    }

    #[test]
    fn huge_jump_starts_at_most_one_event_without_replay() {
        let mut l = layer();
        tick(&mut l, 0, 6000);
        let far = 9_007_199_254_730_000;
        assert_eq!(tick(&mut l, far, 6000), 0);
        assert_eq!(l.phase, Phase::Active { start: far });
        assert_eq!(tick(&mut l, far + 600, 6000), 48);
        assert_eq!(tick(&mut l, far + 3200, 6000), 0);
        assert_eq!(l.phase, Phase::Idle);
        assert_eq!(l.cooldown_until, Some(far + 11_200));
    }

    #[test]
    fn cancellation_starts_cooldown_at_the_frame_time() {
        let mut l = layer();
        tick(&mut l, 0, 6000);
        assert_eq!(tick(&mut l, 1200, 6000), 96);
        // Low strength cancels now.
        assert_eq!(tick(&mut l, 1300, 1499), 0);
        assert_eq!(l.cooldown_until, Some(9300));
        assert_eq!(tick(&mut l, 9299, 6000), 0);
        assert_eq!(l.phase, Phase::Idle);
        assert_eq!(tick(&mut l, 9300, 6000), 0);
        assert_eq!(l.phase, Phase::Active { start: 9300 });
    }

    #[test]
    fn clear_head_cancels_and_cools_down_from_the_next_frame() {
        let mut l = layer();
        tick(&mut l, 0, 6000);
        tick(&mut l, 1200, 6000);
        l.clear_head();
        assert_eq!(l.opacity(), 0);
        assert_eq!(l.phase, Phase::Idle);
        assert_eq!(tick(&mut l, 1500, 6000), 0);
        assert_eq!(l.cooldown_until, Some(9500));
        assert_eq!(tick(&mut l, 9499, 6000), 0);
        assert_eq!(l.phase, Phase::Idle);
        tick(&mut l, 9500, 6000);
        assert_eq!(l.phase, Phase::Active { start: 9500 });
    }

    #[test]
    fn placement_confidence_and_age_boundaries() {
        let mut l = layer();
        l.observe(W, H, 0, 0, 2, 2, 799, 100).unwrap();
        assert!(l.placement(100).is_none());
        l.observe(W, H, 0, 0, 2, 2, 800, 100).unwrap();
        assert!(l.placement(99).is_none(), "future observation");
        assert!(l.placement(100).is_some());
        assert!(l.placement(600).is_some());
        assert!(l.placement(601).is_none());
    }

    #[test]
    fn invalid_observations_change_nothing() {
        let mut l = layer();
        l.observe(W, H, 1, 1, 2, 2, 900, 50).unwrap();
        let before = l.observation;
        for (x, y, w, h, c, t) in [
            (0, 0, 0, 1, 900, 60),
            (0, 0, 1, 0, 900, 60),
            (-1, 0, 1, 1, 900, 60),
            (0, -1, 1, 1, 900, 60),
            (7, 0, 2, 1, 900, 60),
            (0, 7, 1, 2, 900, 60),
            (i32::MAX, 0, u32::MAX, 1, 900, 60),
            (0, 0, 1, 1, 1001, 60),
            (0, 0, 1, 1, 900, 49),
        ] {
            assert!(l.observe(W, H, x, y, w, h, c, t).is_err());
            assert_eq!(l.observation, before);
            assert_eq!(l.last_observed_ms, Some(50));
        }
        l.observe(W, H, 0, 0, 8, 8, 0, 50).unwrap();
        l.observe(W, H, 0, 0, 8, 8, 1000, 50).unwrap();
    }

    #[test]
    fn reset_and_disable_clear_everything_but_keep_the_scratch() {
        let mut l = layer();
        tick(&mut l, 0, 6000);
        tick(&mut l, 1200, 6000);
        l.reset();
        assert_eq!(l.opacity(), 0);
        assert!(l.observation.is_none() && l.cooldown_until.is_none());
        assert!(l.last_observed_ms.is_none());
        l.observe(W, H, 0, 0, 1, 1, 1000, 0).unwrap();
        l.configure(true, W * H).unwrap();
        assert!(l.observation.is_some(), "identical settings keep state");
        l.configure(false, W * H).unwrap();
        assert!(l.observation.is_none());
        assert_eq!(l.scratch.len(), W * H * 4);
    }

    #[test]
    fn scratch_is_allocated_once_and_reused() {
        let mut l = layer();
        let (pointer, capacity) = (l.scratch.as_ptr(), l.scratch.capacity());
        assert_eq!(capacity, W * H * 4);
        for now in (0..4000).step_by(100) {
            tick(&mut l, now, 6000);
        }
        l.configure(false, W * H).unwrap();
        l.configure(true, W * H).unwrap();
        for now in (0..4000).step_by(100) {
            tick(&mut l, now, 6000);
        }
        assert_eq!(l.scratch.as_ptr(), pointer);
        assert_eq!(l.scratch.capacity(), capacity);
    }

    #[test]
    fn allocation_failure_preserves_settings_and_state() {
        let mut l = SkullLayer::new();
        assert!(l.configure(true, usize::MAX).is_err());
        assert!(!l.enabled && l.scratch.is_empty());
        assert!(l.configure(true, usize::MAX / 4).is_err());
        assert!(!l.enabled && l.scratch.is_empty());
        let mut on = layer();
        on.observe(W, H, 0, 0, 1, 1, 1000, 5).unwrap();
        assert!(
            on.configure(true, usize::MAX).is_ok(),
            "no-op when identical"
        );
        assert!(on.observation.is_some());
    }

    #[test]
    fn render_failure_commits_no_state() {
        let mut l = layer();
        tick(&mut l, 0, 6000);
        let phase = l.phase;
        l.observe(W, H, 0, 0, 2, 2, 1000, 1200).unwrap();
        // Wrong output length is rejected before drawing or committing.
        let mut short = vec![0; 4];
        assert!(l.apply(&mut short, W, H, 1200, 6000, false).is_err());
        assert_eq!(l.phase, phase);
        assert_eq!(l.opacity(), 0);
    }
}
