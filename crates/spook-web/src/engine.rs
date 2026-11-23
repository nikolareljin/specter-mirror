//! Host-testable browser frame engine built on the shared spook-core functions.
use spook_core::{bayer_dither_gray8, grayscale_rgb24, sobel_l1, RgbFrame};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

pub const MAX_WIDTH: u32 = 640;
pub const MAX_HEIGHT: u32 = 480;
pub const MODE_MIRROR: u8 = 0;
pub const MODE_SPECTRAL: u8 = 1;
pub const MODE_PORTRAIT: u8 = 2;

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
        if mode > MODE_PORTRAIT {
            return Err(format!("unsupported mode {mode}"));
        }
        let pixels = self.gray.len();
        let expected = pixels.checked_mul(4).ok_or("frame byte count overflows")?;
        if input.len() != expected {
            return Err(format!(
                "input length {} does not match expected {expected}",
                input.len()
            ));
        }
        let (rgb_pixels, _) = self.rgb.as_chunks_mut::<3>();
        let (rgba_pixels, _) = input.as_chunks::<4>();
        for (rgb, rgba) in rgb_pixels.iter_mut().zip(rgba_pixels) {
            rgb.copy_from_slice(&rgba[..3]);
        }
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
