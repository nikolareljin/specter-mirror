//! Host-testable browser frame engine built on the shared spook-core functions.
use spook_core::{
    bayer_dither_gray8, grayscale_rgb24, sobel_l1, RgbFrame, TemporalHistory, TemporalOptions,
};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

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
        if (delay_ms > 0 || trail_strength > 0) && self.temporal.is_none() {
            let history =
                TemporalHistory::new(self.width, self.height).map_err(|e| e.to_string())?;
            let mut work = Vec::new();
            work.try_reserve_exact(self.gray.len() * 4)
                .map_err(|_| "temporal working-frame allocation failed")?;
            work.resize(self.gray.len() * 4, 0);
            self.work = work;
            self.temporal = Some(history);
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
        if !elapsed_ms.is_finite()
            || elapsed_ms.fract() != 0.0
            || !(0.0..=MAX_SAFE_MS).contains(&elapsed_ms)
        {
            return Err("elapsed time must be an integer from 0 to 9007199254740991".into());
        }
        let now = elapsed_ms as u64;
        if self.last_time_ms.is_some_and(|last| now < last) {
            return Err("elapsed time must not decrease".into());
        }
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

fn fill_rgb(rgb: &mut [u8], rgba: &[u8]) {
    let (rgb_pixels, _) = rgb.as_chunks_mut::<3>();
    let (rgba_pixels, _) = rgba.as_chunks::<4>();
    for (dst, src) in rgb_pixels.iter_mut().zip(rgba_pixels) {
        dst.copy_from_slice(&src[..3]);
    }
}
