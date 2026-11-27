//! Head observations and the skull apparition for the browser engine.
//!
//! The layer owns one fallibly allocated RGBA scratch frame and fixed
//! metadata. It never infers a head from motion: bounds come only from
//! `observe`, in unmirrored processing-frame pixels. It does not select or
//! schedule events. The `HauntDirector` decides when SKULL is selected and
//! owns eligibility and cooldowns; this layer only draws one short apparition
//! for each director event that selects SKULL.
//!
//! # Placement
//!
//! An observation is usable at processing time `now` only when the layer is
//! enabled, its confidence is at least 800 (of 1000), `observed <= now`, and
//! `now - observed <= 500` ms. Order is checked before subtraction. The same
//! test feeds the director's `head_available` and the renderer. Bounds are
//! stored unmirrored and transformed once, at render time:
//! `output_x = frame_width - x - width`.
//!
//! # Apparition
//!
//! The apparition starts at the first frame whose `DirectorSnapshot` selects
//! SKULL with positive strength, at `t = 0`:
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
//! integers. `strength` is the snapshot's strength, which already contains the
//! progression strength; it is not multiplied by progression again. The
//! maximum opacity is 96 (the core renderer accepts up to 128). The director's
//! selection frame carries a neutral snapshot, so it never draws a skull.
//!
//! # One apparition per director event
//!
//! The snapshot carries no event id. The identity of an event is recovered
//! from the director's published policy instead: a SKULL event lasts 4000 to
//! 8000 ms, and a new event using the SKULL layer cannot be selected until
//! 8000 ms after the previous one finished, which is itself no earlier than
//! the previous event's last positive snapshot. Consider two positive SKULL
//! snapshots at `p < t` with `p` the latest previous one:
//!
//! * the same event: both lie inside one event, so `t - p < 8000`;
//! * different events: `t - p > 8000`.
//!
//! The layer therefore keeps `last_positive` and a `consumed` latch. A
//! positive SKULL snapshot with `t - last_positive >= 8000` (or none yet)
//! begins a new event and clears the latch; any other positive snapshot is the
//! same event. The latch is set by the first positive snapshot and stays set
//! when the 3200 ms window ends, when strength rounds to zero (a neutral
//! snapshot changes neither `last_positive` nor the latch) and when the head
//! briefly returns. Repeated or equal timestamps give `t - last_positive = 0`
//! and cannot rearm. Large jumps are one frame: at most one apparition starts,
//! and missed ones are never replayed.
//!
//! # Cancellation
//!
//! A neutral snapshot, reduced motion, an unusable placement or a disabled
//! layer draws nothing. `clear_head` and a real enable/disable change drop the
//! observation and the apparition at once and make the next frame report the
//! head as unavailable to the director exactly once. The director then ends
//! any active event and applies its normal cooldown, even if the host
//! observes a head again before that frame. The director is never reset.
use spook_core::{
    render_skull_rgba, DirectorSnapshot, OverlayRect, SkullOptions, HAUNT_FULL_STRENGTH, SKULL,
};

pub(crate) const MIN_CONFIDENCE: u16 = 800;
pub(crate) const MAX_CONFIDENCE: u16 = 1000;
pub(crate) const MAX_AGE_MS: u64 = 500;
pub(crate) const FADE_MS: u64 = 1200;
pub(crate) const HOLD_MS: u64 = 800;
pub(crate) const EVENT_MS: u64 = FADE_MS + HOLD_MS + FADE_MS;
/// Minimum gap between positive SKULL snapshots of different director events
/// (the director's per-layer cooldown, exclusive of the event itself).
pub(crate) const NEW_EVENT_GAP_MS: u64 = 8000;
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

/// Apparition bookkeeping for the current director event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Event {
    /// Time of the latest positive SKULL snapshot.
    last_positive: Option<u64>,
    /// Start of the consumed apparition; set once per director event.
    start: Option<u64>,
}

struct Step {
    event: Event,
    opacity: u8,
}

pub(crate) struct SkullLayer {
    enabled: bool,
    scratch: Vec<u8>,
    observation: Option<Observation>,
    last_observed_ms: Option<u64>,
    event: Event,
    /// Report the head as unavailable to the director on the next frame.
    unavailable_once: bool,
    opacity: u8,
}

/// Skull opacity for `elapsed` ms into an apparition at the given strength.
pub(crate) fn opacity_at(elapsed: u64, strength: u16) -> u8 {
    let ramp = if elapsed < FADE_MS {
        elapsed
    } else if elapsed < FADE_MS + HOLD_MS {
        FADE_MS
    } else {
        // Fade out; zero once the apparition has ended.
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
            event: Event::default(),
            unavailable_once: false,
            opacity: 0,
        }
    }

    pub fn opacity(&self) -> u8 {
        self.opacity
    }

    /// Enables or disables the layer. The scratch frame (`pixels * 4` bytes)
    /// is allocated here, once. Allocation failure changes nothing. Identical
    /// settings keep all state; a real change clears observations and the
    /// apparition and makes the director see no head on the next frame.
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
        self.unavailable_once = true;
        Ok(())
    }

    /// Clears observations, the apparition and opacity; keeps settings. Used
    /// with a director reset, so no pending cancellation remains.
    pub fn reset(&mut self) {
        self.observation = None;
        self.last_observed_ms = None;
        self.event = Event::default();
        self.unavailable_once = false;
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

    /// Forgets the observation and hides the skull at once. The next frame
    /// reports no head to the director, which cancels any active event.
    pub fn clear_head(&mut self) {
        self.observation = None;
        self.unavailable_once = true;
        self.opacity = 0;
    }

    fn placement(&self, now: u64) -> Option<Observation> {
        if !self.enabled {
            return None;
        }
        let seen = self.observation?;
        if seen.confidence < MIN_CONFIDENCE || seen.observed_ms > now {
            return None;
        }
        (now - seen.observed_ms <= MAX_AGE_MS).then_some(seen)
    }

    /// Whether the director may select SKULL at `now`. Uses the same
    /// validated placement the renderer uses.
    pub fn head_available(&self, now: u64) -> bool {
        !self.unavailable_once && self.placement(now).is_some()
    }

    fn step(&self, now: u64, snapshot: DirectorSnapshot, reduced: bool) -> Step {
        let mut event = self.event;
        let selected = snapshot.layers & SKULL != 0 && snapshot.strength > 0;
        let usable = !reduced && self.head_available(now);
        if !selected {
            // A neutral or SKULL-free snapshot changes no event identity.
            return Step { event, opacity: 0 };
        }
        if event
            .last_positive
            .is_none_or(|last| now.saturating_sub(last) >= NEW_EVENT_GAP_MS)
        {
            event.start = None;
        }
        event.last_positive = Some(now);
        if usable && event.start.is_none() {
            event.start = Some(now);
        }
        let opacity = match event.start {
            Some(start) if usable && now.saturating_sub(start) < EVENT_MS => {
                opacity_at(now.saturating_sub(start), snapshot.strength)
            }
            _ => 0,
        };
        Step { event, opacity }
    }

    /// Advances the apparition for one valid automatic frame and, when
    /// visible, draws the skull over `out` (the final mirrored RGBA). State is
    /// committed only after drawing succeeds.
    pub fn apply(
        &mut self,
        out: &mut [u8],
        width: usize,
        height: usize,
        now: u64,
        snapshot: DirectorSnapshot,
        reduced: bool,
    ) -> Result<(), String> {
        let step = self.step(now, snapshot, reduced);
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
        self.event = step.event;
        self.unavailable_once = false;
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
        layer.unavailable_once = false;
        layer
    }

    fn frame() -> Vec<u8> {
        vec![40; W * H * 4]
    }

    fn selected(strength: u16) -> DirectorSnapshot {
        DirectorSnapshot {
            layers: SKULL,
            strength,
            delay_ms: 0,
            trail_strength: 0,
        }
    }

    /// Observe at `now` (age 0) then process a snapshot; returns the opacity.
    fn tick(layer: &mut SkullLayer, now: u64, snapshot: DirectorSnapshot) -> u8 {
        layer.observe(W, H, 1, 1, 4, 5, 1000, now).unwrap();
        layer
            .apply(&mut frame(), W, H, now, snapshot, false)
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
    fn apparition_starts_at_the_first_positive_snapshot_and_lasts_3200_ms() {
        let mut l = layer();
        // Neutral frames (including the director's selection frame) draw nothing.
        assert_eq!(tick(&mut l, 500, DirectorSnapshot::default()), 0);
        assert_eq!(l.event, Event::default());
        for (offset, expected) in [
            (0, 0),
            (1, 0),
            (600, 48),
            (1199, 96),
            (1200, 96),
            (1999, 96),
            (2000, 96),
            (2600, 48),
            (3199, 0),
            (3200, 0),
            (3201, 0),
            (7999, 0),
        ] {
            assert_eq!(
                tick(&mut l, 1000 + offset, selected(6000)),
                expected,
                "{offset}"
            );
            assert_eq!(l.event.start, Some(1000));
        }
    }

    #[test]
    fn repeated_timestamps_and_zero_strength_never_rearm() {
        let mut l = layer();
        for _ in 0..3 {
            assert_eq!(tick(&mut l, 1000, selected(6000)), 0);
        }
        assert_eq!(tick(&mut l, 1600, selected(6000)), 48);
        assert_eq!(tick(&mut l, 1600, selected(6000)), 48);
        // Strength rounding to zero is a neutral snapshot inside the event.
        assert_eq!(tick(&mut l, 2000, DirectorSnapshot::default()), 0);
        assert_eq!(tick(&mut l, 2400, selected(6000)), 96);
        assert_eq!(l.event.start, Some(1000));
        // After the window the latch holds for the rest of the event.
        assert_eq!(tick(&mut l, 4300, DirectorSnapshot::default()), 0);
        assert_eq!(tick(&mut l, 5000, selected(6000)), 0);
        assert_eq!(tick(&mut l, 8999, selected(6000)), 0);
        assert_eq!(l.event.start, Some(1000));
    }

    #[test]
    fn a_new_director_event_needs_a_gap_of_8000_ms_between_positive_snapshots() {
        let mut l = layer();
        tick(&mut l, 1000, selected(6000));
        tick(&mut l, 2000, selected(6000));
        // 7999 ms after the last positive snapshot is still the same event.
        assert_eq!(tick(&mut l, 9999, selected(6000)), 0);
        assert_eq!(l.event.start, Some(1000));
        // 8000 ms after it a different event is selected; elapsed starts at 0.
        assert_eq!(tick(&mut l, 17_999, selected(6000)), 0);
        assert_eq!(l.event.start, Some(17_999));
        assert_eq!(tick(&mut l, 18_599, selected(6000)), 48);
    }

    #[test]
    fn huge_jump_starts_at_most_one_apparition_without_replay() {
        let mut l = layer();
        tick(&mut l, 0, selected(6000));
        let far = 9_007_199_254_730_000;
        assert_eq!(tick(&mut l, far, selected(6000)), 0);
        assert_eq!(l.event.start, Some(far));
        assert_eq!(tick(&mut l, far + 600, selected(6000)), 48);
        assert_eq!(tick(&mut l, far + 3200, selected(6000)), 0);
        assert_eq!(tick(&mut l, far + 5000, selected(6000)), 0);
        assert_eq!(l.event.start, Some(far));
    }

    #[test]
    fn no_placement_reduced_motion_or_a_disabled_layer_draw_nothing() {
        let mut l = layer();
        tick(&mut l, 0, selected(6000));
        assert_eq!(tick(&mut l, 1200, selected(6000)), 96);
        // Reduced motion hides at once; the consumed latch stays.
        l.apply(&mut frame(), W, H, 1300, selected(6000), true)
            .unwrap();
        assert_eq!(l.opacity(), 0);
        assert_eq!(tick(&mut l, 1400, selected(6000)), 96);
        // Age 501 hides, 500 shows.
        l.observe(W, H, 1, 1, 4, 5, 1000, 2000).unwrap();
        l.apply(&mut frame(), W, H, 2500, selected(6000), false)
            .unwrap();
        // Elapsed 2500 is 700 ms into the fade out: 96 * 700 / 1200 = 56.
        assert_eq!(l.opacity(), 56);
        l.apply(&mut frame(), W, H, 2501, selected(6000), false)
            .unwrap();
        assert_eq!(l.opacity(), 0);
        // Recovery of the head inside the same event cannot restart it.
        assert_eq!(tick(&mut l, 3300, selected(6000)), 0);
        assert_eq!(l.event.start, Some(0));
        let mut off = layer();
        off.configure(false, W * H).unwrap();
        assert!(!off.head_available(0));
    }

    #[test]
    fn clear_head_and_toggles_hide_at_once_and_report_no_head_once() {
        let mut l = layer();
        tick(&mut l, 0, selected(6000));
        tick(&mut l, 1200, selected(6000));
        l.clear_head();
        assert_eq!(l.opacity(), 0);
        // A head observed again before the next frame is still reported
        // missing once, so the director cancels the consumed event.
        l.observe(W, H, 1, 1, 4, 5, 1000, 1300).unwrap();
        assert!(!l.head_available(1300));
        l.apply(&mut frame(), W, H, 1300, DirectorSnapshot::default(), false)
            .unwrap();
        assert!(l.head_available(1300));
        l.configure(false, W * H).unwrap();
        assert!(!l.head_available(1300) && l.observation.is_none());
        l.configure(true, W * H).unwrap();
        assert!(!l.head_available(1300));
        assert_eq!(l.event, Event::default());
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
        assert!(l.head_available(600) && !l.head_available(601));
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
        tick(&mut l, 0, selected(6000));
        tick(&mut l, 1200, selected(6000));
        l.reset();
        assert_eq!(l.opacity(), 0);
        assert!(l.observation.is_none() && l.event == Event::default());
        assert!(l.last_observed_ms.is_none() && !l.unavailable_once);
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
            tick(&mut l, now, selected(6000));
        }
        l.configure(false, W * H).unwrap();
        l.configure(true, W * H).unwrap();
        for now in (0..4000).step_by(100) {
            tick(&mut l, now, selected(6000));
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
        tick(&mut l, 0, selected(6000));
        let event = l.event;
        l.observe(W, H, 0, 0, 2, 2, 1000, 1200).unwrap();
        // Wrong output length is rejected before drawing or committing.
        let mut short = vec![0; 4];
        assert!(l
            .apply(&mut short, W, H, 1200, selected(6000), false)
            .is_err());
        assert_eq!(l.event, event);
        assert_eq!(l.opacity(), 0);
    }
}
