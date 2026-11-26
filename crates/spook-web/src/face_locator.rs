//! Optional local frontal-face locator for the browser worker.
//!
//! Wraps rustface (SeetaFace, BSD-2-Clause) with the checked model already in
//! `assets/models`. It reports face bounds only; the page expands them to head
//! bounds once. There is no identity recognition. The detector allocates
//! internally while it runs; this adapter only reuses the detector itself.
use rustface::{Detector, FaceInfo, ImageData};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

/// Largest accepted detector image, matching the worker's offered frame.
pub const MAX_DETECT_WIDTH: u32 = 160;
pub const MAX_DETECT_HEIGHT: u32 = 120;

const MODEL: &[u8] = include_bytes!("../../../assets/models/seeta_fd_frontal_v1.0.bin");

// Same detector settings as the desktop preview, which runs on 160 x 120.
const MIN_FACE_SIZE: u32 = 30;
const SCORE_THRESHOLD: f64 = 2.0;
const PYRAMID_SCALE: f32 = 0.8;
const WINDOW_STEP: u32 = 4;

/// One detector candidate in detector-image pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Candidate {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    score: f64,
}

/// Maps an accepted score (>= 2) to 800..=1000, as the desktop preview does.
fn confidence(score: f64) -> u32 {
    (800.0 + (score - SCORE_THRESHOLD).max(0.0) * 50.0).min(1000.0) as u32
}

/// Clips to the image and returns `(x, y, width, height)`; `None` if empty.
fn clip(c: &Candidate, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let left = i64::from(c.x).max(0);
    let top = i64::from(c.y).max(0);
    let right = (i64::from(c.x) + i64::from(c.width)).min(i64::from(width));
    let bottom = (i64::from(c.y) + i64::from(c.height)).min(i64::from(height));
    (right > left && bottom > top).then(|| {
        (
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        )
    })
}

/// Picks the largest accepted face by clipped area. Equal areas go to the
/// smaller y, then smaller x, then higher score, so input order is irrelevant.
fn select(candidates: &[Candidate], width: u32, height: u32) -> Vec<u32> {
    let mut best: Option<((u32, u32, u32, u32), f64)> = None;
    for c in candidates {
        if !c.score.is_finite() || c.score < SCORE_THRESHOLD {
            continue;
        }
        let Some(r) = clip(c, width, height) else {
            continue;
        };
        let better = match best {
            None => true,
            Some((b, s)) => {
                let (area, best_area) = (
                    u64::from(r.2) * u64::from(r.3),
                    u64::from(b.2) * u64::from(b.3),
                );
                area.cmp(&best_area)
                    .then(b.1.cmp(&r.1))
                    .then(b.0.cmp(&r.0))
                    .then(c.score.total_cmp(&s))
                    .then(b.2.cmp(&r.2))
                    .then(b.3.cmp(&r.3))
                    .is_gt()
            }
        };
        if better {
            best = Some((r, c.score));
        }
    }
    match best {
        Some(((x, y, w, h), score)) => vec![x, y, w, h, confidence(score)],
        None => Vec::new(),
    }
}

/// One reusable frontal-face detector for a fixed grayscale frame size.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct BrowserFaceDetector {
    width: u32,
    height: u32,
    detector: Box<dyn Detector>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl BrowserFaceDetector {
    /// Accepts nonzero dimensions up to 160 x 120.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new(width: u32, height: u32) -> Result<BrowserFaceDetector, String> {
        if width == 0 || height == 0 {
            return Err("detector dimensions must be nonzero".into());
        }
        if width > MAX_DETECT_WIDTH || height > MAX_DETECT_HEIGHT {
            return Err(format!(
                "detector dimensions must not exceed {MAX_DETECT_WIDTH} x {MAX_DETECT_HEIGHT}"
            ));
        }
        let model =
            rustface::read_model(MODEL).map_err(|e| format!("cannot read the face model: {e}"))?;
        let mut detector = rustface::create_detector_with_model(model);
        detector.set_min_face_size(MIN_FACE_SIZE);
        detector.set_score_thresh(SCORE_THRESHOLD);
        detector.set_pyramid_scale_factor(PYRAMID_SCALE);
        detector.set_slide_window_step(WINDOW_STEP, WINDOW_STEP);
        Ok(Self {
            width,
            height,
            detector,
        })
    }

    /// Detects in one tightly packed grayscale frame (`width * height` bytes).
    /// Returns an empty vector, or `[x, y, width, height, confidence]` for the
    /// largest accepted face in detector-image pixels; confidence is 800..=1000.
    pub fn detect_gray8(&mut self, pixels: &[u8]) -> Result<Vec<u32>, String> {
        let expected = self.width as usize * self.height as usize;
        if pixels.len() != expected {
            return Err(format!(
                "expected {expected} grayscale bytes, got {}",
                pixels.len()
            ));
        }
        let image = ImageData::new(pixels, self.width, self.height);
        let found: Vec<Candidate> = self
            .detector
            .detect(&image)
            .iter()
            .map(|f: &FaceInfo| {
                let b = f.bbox();
                Candidate {
                    x: b.x(),
                    y: b.y(),
                    width: b.width(),
                    height: b.height(),
                    score: f.score(),
                }
            })
            .collect();
        Ok(select(&found, self.width, self.height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(x: i32, y: i32, width: u32, height: u32, score: f64) -> Candidate {
        Candidate {
            x,
            y,
            width,
            height,
            score,
        }
    }

    #[test]
    fn picks_largest_clipped_area() {
        let faces = [c(10, 10, 30, 30, 5.0), c(60, 20, 40, 40, 2.5)];
        assert_eq!(select(&faces, 160, 120), vec![60, 20, 40, 40, 825]);
    }

    #[test]
    fn clips_to_the_image_before_comparing() {
        // 50 x 50 hangs off the left and top edge: clipped area 30 x 30 = 900.
        let faces = [c(-20, -20, 50, 50, 9.0), c(100, 50, 31, 30, 2.0)];
        assert_eq!(select(&faces, 160, 120), vec![100, 50, 31, 30, 800]);
        assert_eq!(select(&faces[..1], 160, 120), vec![0, 0, 30, 30, 1000]);
    }

    #[test]
    fn equal_area_ties_ignore_input_order() {
        let a = c(80, 40, 30, 30, 3.0);
        let b = c(20, 40, 30, 30, 9.0);
        let top = c(90, 10, 30, 30, 2.0);
        for order in [[a, b, top], [top, b, a], [b, a, top]] {
            assert_eq!(select(&order, 160, 120), vec![90, 10, 30, 30, 800]);
        }
        assert_eq!(select(&[a, b], 160, 120), select(&[b, a], 160, 120));
        assert_eq!(select(&[a, b], 160, 120), vec![20, 40, 30, 30, 1000]);
    }

    #[test]
    fn rejects_low_score_empty_and_nonfinite() {
        let faces = [
            c(10, 10, 40, 40, 1.99),
            c(200, 10, 40, 40, 9.0),
            c(10, 10, 0, 40, 9.0),
            c(10, 10, 40, 40, f64::NAN),
        ];
        assert!(select(&faces, 160, 120).is_empty());
    }

    #[test]
    fn equal_position_score_and_area_break_ties_by_dimensions() {
        let a = c(20, 10, 20, 40, 3.0);
        let b = c(20, 10, 40, 20, 3.0);
        assert_eq!(select(&[a, b], 160, 120), select(&[b, a], 160, 120));
        assert_eq!(select(&[b, a], 160, 120), vec![20, 10, 20, 40, 850]);
    }

    #[test]
    fn confidence_mapping_is_bounded() {
        assert_eq!(confidence(2.0), 800);
        assert_eq!(confidence(1.0), 800);
        assert_eq!(confidence(3.0), 850);
        assert_eq!(confidence(4.0), 900);
        assert_eq!(confidence(50.0), 1000);
    }
}
