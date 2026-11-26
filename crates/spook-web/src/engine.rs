//! Host-testable browser frame engine built on the shared spook-core functions.
use spook_core::{
    bayer_dither_gray8, grayscale_rgb24, sobel_l1, HauntState, RgbFrame, TemporalHistory,
    TemporalOptions, HAUNT_FULL_STRENGTH,
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[path = "automatic.rs"]
mod automatic;
use automatic::Automatic;
#[path = "skull_event.rs"]
mod skull_event;
use skull_event::SkullLayer;

pub const MAX_WIDTH: u32 = 640;
pub const MAX_HEIGHT: u32 = 480;
pub const MODE_MIRROR: u8 = 0;
pub const MODE_SPECTRAL: u8 = 1;
pub const MODE_PORTRAIT: u8 = 2;
const MAX_DELAY_MS: u16 = 600;
/// Largest integer a JavaScript number represents exactly (2^53 - 1).
const MAX_SAFE_MS: f64 = 9_007_199_254_740_991.0;

const SPECTRAL_CYAN: [u8; 3] = [114, 231, 214];
const BLACK: [u8; 3] = [0, 0, 0];
const BONE: [u8; 3] = [229, 223, 209];
const MIDNIGHT: [u8; 3] = [8, 13, 20];

/// Owns reusable RGB, grayscale, edge and dither buffers for one frame size.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct MirrorEngine {
    width: usize,
    height: usize,
    rgb: Vec<u8>,
    gray: Vec<u8>,
    edges: Vec<u8>,
    dither: Vec<u8>,
    // Temporal state. History and the RGBA working frame are allocated once,
    // when a nonzero option is first enabled, and are never allocated in
    // processing.
    temporal: Option<TemporalHistory>,
    work: Vec<u8>,
    delay_ms: u16,
    trail_strength: u8,
    last_time_ms: Option<u64>,
    automatic: Option<Automatic>,
    automatic_enabled: bool,
    reduced_motion: bool,
    // Optional head-aligned skull layer: one scratch RGBA frame plus metadata.
    skull: SkullLayer,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl MirrorEngine {
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(width: u32, height: u32) -> Result<MirrorEngine, String> {
        if width == 0 || height == 0 {
            return Err("frame dimensions must be nonzero".into());
        }
        if width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(format!(
                "frame dimensions must not exceed {MAX_WIDTH} x {MAX_HEIGHT}"
            ));
        }
        let width = width as usize;
        let height = height as usize;
        let pixels = width
            .checked_mul(height)
            .ok_or("frame pixel count overflows")?;
        let rgb_len = pixels.checked_mul(3).ok_or("frame byte count overflows")?;
        Ok(Self {
            width,
            height,
            rgb: vec![0; rgb_len],
            gray: vec![0; pixels],
            edges: vec![0; pixels],
            dither: vec![0; pixels],
            temporal: None,
            work: Vec::new(),
            delay_ms: 0,
            trail_strength: 0,
            last_time_ms: None,
            automatic: None,
            automatic_enabled: false,
            reduced_motion: false,
            skull: SkullLayer::new(),
        })
    }

    /// Returns mirrored RGBA. `input` must be exactly `width * height * 4` bytes.
    /// Invalid arguments are rejected before any buffer is modified.
    pub fn process_rgba(
        &mut self,
        input: &[u8],
        mode: u8,
        intensity: u8,
    ) -> Result<Vec<u8>, String> {
        let expected = self.validate(input, mode)?;
        fill_rgb(&mut self.rgb, input);
        self.render(expected, mode, intensity)
    }

    /// Sets reflection delay (0..=600 ms) and ghost-trail strength (0..=255).
    /// Invalid values are rejected before any change. Changing either value
    /// clears history and time; identical values change nothing. History is
    /// allocated here the first time an option is enabled, never while
    /// processing. Disabling both clears history and keeps cached storage.
    pub fn configure_temporal(&mut self, delay_ms: u16, trail_strength: u8) -> Result<(), String> {
        if delay_ms > MAX_DELAY_MS {
            return Err(format!("delay must not exceed {MAX_DELAY_MS} ms"));
        }
        if delay_ms == self.delay_ms && trail_strength == self.trail_strength {
            return Ok(());
        }
        if delay_ms > 0 || trail_strength > 0 {
            self.ensure_temporal()?;
        }
        self.delay_ms = delay_ms;
        self.trail_strength = trail_strength;
        self.reset_temporal();
        Ok(())
    }

    /// Keeps settings but forgets history and time, for a new source or epoch.
    pub fn reset_temporal(&mut self) {
        if let Some(history) = self.temporal.as_mut() {
            history.reset();
        }
        self.last_time_ms = None;
        if let Some(automatic) = self.automatic.as_mut() {
            automatic.reset();
        }
        self.skull.reset();
    }

    /// Changes automatic/reduced-motion preferences and resets the session.
    /// Storage is allocated here, not while processing. Identical settings preserve history.
    pub fn configure_automatic(
        &mut self,
        enabled: bool,
        reduced_motion: bool,
    ) -> Result<(), String> {
        if enabled == self.automatic_enabled && reduced_motion == self.reduced_motion {
            return Ok(());
        }
        if enabled {
            let automatic = if self.automatic.is_none() {
                Some(Automatic::new(self.gray.len())?)
            } else {
                None
            };
            self.ensure_temporal()?;
            if automatic.is_some() {
                self.automatic = automatic;
            }
        }
        self.automatic_enabled = enabled;
        self.reduced_motion = reduced_motion;
        self.reset_temporal();
        Ok(())
    }

    /// Uses raw-frame motion and the shared progression before temporal/palette processing.
    pub fn process_automatic_rgba(
        &mut self,
        input: &[u8],
        elapsed_ms: f64,
    ) -> Result<Vec<u8>, String> {
        let expected = self.validate(input, MODE_MIRROR)?;
        let now = self.validate_time(elapsed_ms)?;
        if !self.automatic_enabled {
            return Err("automatic reflection is disabled".into());
        }
        fill_rgb(&mut self.rgb, input);
        let frame = RgbFrame::new(self.width, self.height, self.width * 3, &self.rgb)
            .map_err(|e| e.to_string())?;
        grayscale_rgb24(&frame, &mut self.gray).map_err(|e| e.to_string())?;
        let automatic = self
            .automatic
            .as_mut()
            .ok_or("automatic reflection is not configured")?;
        let level = automatic.update(
            &self.gray,
            self.width,
            self.height,
            now,
            self.reduced_motion,
        )?;
        let scale =
            |maximum: u32| maximum * u32::from(level.strength) / u32::from(HAUNT_FULL_STRENGTH);
        let options = TemporalOptions {
            delay_ms: scale(250) as u16,
            trail_strength: scale(120) as u8,
        };
        let history = self
            .temporal
            .as_mut()
            .ok_or("temporal history is not configured")?;
        history
            .process(input, &mut self.work, now, options)
            .map_err(|e| e.to_string())?;
        if options.delay_ms > 0 || options.trail_strength > 0 {
            fill_rgb(&mut self.rgb, &self.work);
        }
        self.last_time_ms = Some(now);
        let intensity = scale(140) as u8;
        let mode = if intensity == 0 {
            MODE_MIRROR
        } else {
            MODE_SPECTRAL
        };
        let mut output = self.render(expected, mode, intensity)?;
        self.skull.apply(
            &mut output,
            self.width,
            self.height,
            now,
            level.strength,
            self.reduced_motion,
        )?;
        Ok(output)
    }

    /// Draws each visit's clear opening, rise, hold and return from `seed`
    /// (4 to 15 seconds each, eased). The host supplies the seed; the same
    /// seed replays the same timing. Resets the automatic session and skull
    /// state but keeps temporal history storage. Requires `configure_automatic`.
    pub fn configure_seed(&mut self, seed: u32) -> Result<(), String> {
        let automatic = self
            .automatic
            .as_mut()
            .ok_or("automatic reflection is not configured")?;
        automatic.set_seed(seed)?;
        self.reset_temporal();
        Ok(())
    }

    /// Enables or disables the head-aligned skull layer for automatic output.
    /// The scratch frame is allocated here, never while processing; failure
    /// preserves every setting. Identical settings change nothing. A real
    /// change clears skull observations and events, not temporal history or
    /// the automatic envelope.
    pub fn configure_skull(&mut self, enabled: bool) -> Result<(), String> {
        self.skull.configure(enabled, self.gray.len())
    }

    /// Records head bounds in the unmirrored processing frame. The rectangle
    /// must be nonempty and fully inside the frame, confidence 0..=1000, and
    /// `observed_ms` an integer in 0..=9007199254740991 that does not precede
    /// the previous accepted observation (until reset). Errors change no state.
    /// While the layer is disabled a valid observation is accepted and ignored.
    pub fn observe_head(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        confidence: u16,
        observed_ms: f64,
    ) -> Result<(), String> {
        let observed = parse_ms(observed_ms)?;
        self.skull.observe(
            self.width,
            self.height,
            x,
            y,
            width,
            height,
            confidence,
            observed,
        )
    }

    /// Forgets the head bounds and cancels any skull event immediately.
    pub fn clear_head(&mut self) {
        self.skull.clear_head();
    }

    /// Opacity (0..=96) applied to the most recent automatic frame.
    pub fn skull_opacity(&self) -> u8 {
        self.skull.opacity()
    }

    /// State codes: idle=0, presence=1, transforming=2, haunted=3, recovering=4.
    pub fn haunt_state(&self) -> u8 {
        match self
            .automatic
            .as_ref()
            .map(|a| a.level.state)
            .unwrap_or(HauntState::Idle)
        {
            HauntState::Idle => 0,
            HauntState::PresenceDetected => 1,
            HauntState::Transforming => 2,
            HauntState::Haunted => 3,
            HauntState::Recovering => 4,
        }
    }

    pub fn haunt_strength(&self) -> u16 {
        self.automatic.as_ref().map_or(0, |a| a.level.strength)
    }
    pub fn motion_activity(&self) -> u16 {
        self.automatic.as_ref().map_or(0, |a| a.level.activity)
    }

    /// Like `process_rgba`, but applies delayed reflection and ghost trails to
    /// the raw RGBA first. `elapsed_ms` must be a finite integer in
    /// `0..=9007199254740991` and must not decrease between successful calls
    /// (use `reset_temporal` to start a new epoch). Errors change no state.
    pub fn process_timed_rgba(
        &mut self,
        input: &[u8],
        mode: u8,
        intensity: u8,
        elapsed_ms: f64,
    ) -> Result<Vec<u8>, String> {
        let expected = self.validate(input, mode)?;
        let now = self.validate_time(elapsed_ms)?;
        match self.temporal.as_mut() {
            Some(history) if self.delay_ms > 0 || self.trail_strength > 0 => {
                let options = TemporalOptions {
                    delay_ms: self.delay_ms,
                    trail_strength: self.trail_strength,
                };
                history
                    .process(input, &mut self.work, now, options)
                    .map_err(|e| e.to_string())?;
                fill_rgb(&mut self.rgb, &self.work);
            }
            _ => fill_rgb(&mut self.rgb, input),
        }
        self.last_time_ms = Some(now);
        self.render(expected, mode, intensity)
    }
}

impl MirrorEngine {
    fn ensure_temporal(&mut self) -> Result<(), String> {
        if self.temporal.is_none() {
            let history =
                TemporalHistory::new(self.width, self.height).map_err(|e| e.to_string())?;
            let mut work = Vec::new();
            work.try_reserve_exact(self.gray.len() * 4)
                .map_err(|_| "temporal working-frame allocation failed")?;
            work.resize(self.gray.len() * 4, 0);
            self.work = work;
            self.temporal = Some(history);
        }
        Ok(())
    }

    fn validate_time(&self, elapsed_ms: f64) -> Result<u64, String> {
        let now = parse_ms(elapsed_ms)?;
        if self.last_time_ms.is_some_and(|last| now < last) {
            return Err("elapsed time must not decrease".into());
        }
        Ok(now)
    }
    /// Checks mode and exact RGBA length; returns the expected byte count.
    fn validate(&self, input: &[u8], mode: u8) -> Result<usize, String> {
        if mode > MODE_PORTRAIT {
            return Err(format!("unsupported mode {mode}"));
        }
        let expected = self
            .gray
            .len()
            .checked_mul(4)
            .ok_or("frame byte count overflows")?;
        if input.len() != expected {
            return Err(format!(
                "input length {} does not match expected {expected}",
                input.len()
            ));
        }
        Ok(expected)
    }

    /// Runs palette/edge processing on `self.rgb` and mirrors once.
    fn render(&mut self, expected: usize, mode: u8, intensity: u8) -> Result<Vec<u8>, String> {
        let (w, h) = (self.width, self.height);
        let (map, lit, dark) = match mode {
            MODE_SPECTRAL => {
                let frame = RgbFrame::new(w, h, w * 3, &self.rgb).map_err(|e| e.to_string())?;
                grayscale_rgb24(&frame, &mut self.gray).map_err(|e| e.to_string())?;
                sobel_l1(&self.gray, w, h, w, &mut self.edges).map_err(|e| e.to_string())?;
                (Some(&self.edges), SPECTRAL_CYAN, BLACK)
            }
            MODE_PORTRAIT => {
                let frame = RgbFrame::new(w, h, w * 3, &self.rgb).map_err(|e| e.to_string())?;
                grayscale_rgb24(&frame, &mut self.gray).map_err(|e| e.to_string())?;
                bayer_dither_gray8(&self.gray, w, h, w, &mut self.dither)
                    .map_err(|e| e.to_string())?;
                (Some(&self.dither), BONE, MIDNIGHT)
            }
            _ => (None, BLACK, BLACK),
        };
        let weight = u32::from(intensity);
        let mut out = Vec::with_capacity(expected);
        for y in 0..h {
            for x in 0..w {
                // Core output keeps source orientation; mirror once here.
                let src = y * w + (w - 1 - x);
                let original = &self.rgb[src * 3..src * 3 + 3];
                match map {
                    None => out.extend_from_slice(original),
                    Some(plane) => {
                        let level = u32::from(plane[src]);
                        let target = std::array::from_fn::<_, 3, _>(|channel| {
                            (u32::from(dark[channel]) * (255 - level)
                                + u32::from(lit[channel]) * level
                                + 127)
                                / 255
                        });
                        for (&o, &p) in original.iter().zip(target.iter()) {
                            let mixed = (u32::from(o) * (255 - weight) + p * weight + 127) / 255;
                            out.push(mixed as u8);
                        }
                    }
                }
                out.push(255);
            }
        }
        Ok(out)
    }
}

/// Accepts a finite integer from 0 to 2^53 - 1 (a JavaScript safe integer).
fn parse_ms(elapsed_ms: f64) -> Result<u64, String> {
    if !elapsed_ms.is_finite()
        || elapsed_ms.fract() != 0.0
        || !(0.0..=MAX_SAFE_MS).contains(&elapsed_ms)
    {
        return Err("elapsed time must be an integer from 0 to 9007199254740991".into());
    }
    Ok(elapsed_ms as u64)
}

fn fill_rgb(rgb: &mut [u8], rgba: &[u8]) {
    let (rgb_pixels, _) = rgb.as_chunks_mut::<3>();
    let (rgba_pixels, _) = rgba.as_chunks::<4>();
    for (dst, src) in rgb_pixels.iter_mut().zip(rgba_pixels) {
        dst.copy_from_slice(&src[..3]);
    }
}
