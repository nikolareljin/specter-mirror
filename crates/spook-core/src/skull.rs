//! Translucent procedural skull overlay for caller-supplied bounds.
//!
//! The renderer paints an original skull inside one rectangle. It does not
//! find a head, and it does not mirror, scale or distort the frame. Hosts own
//! localization, timing and intensity envelopes.
//!
//! # Coordinate convention
//!
//! Frame pixels use x to the right and y downward from the top-left corner.
//! `OverlayRect` is the signed top-left corner plus a size, covering
//! `x..x + width` and `y..y + height`. Any `i32` position is accepted. The
//! rectangle is intersected with the frame before any mask arithmetic, so only
//! the visible part is visited. The skull keeps the geometry of the full
//! rectangle: clipping crops it and never rescales it.
//!
//! For rectangle-local pixel `(i, j)` of a `w x h` rectangle, the mask samples
//! the pixel center in integer fixed point (truncating division):
//!
//! ```text
//! u = (2i + 1 - w) * 1000 / w      signed, -999..=999, 0 at the centerline
//! v = (2j + 1) * 500 / h           0 at the top, 999 at the bottom
//! a = |u|                          the mask is left-right symmetric
//! ```
//!
//! Shapes below are in these units, so they scale with the rectangle.
//!
//! * Cranium (`v < 450`): ellipse centered at `(0, 450)` with radii
//!   `(980, 450)`.
//! * Cheek and jaw (`v >= 450`): inside while `a <= hw(v)`, where `hw` falls
//!   linearly from 980 at `v = 450` to 700 at `v = 650`, then to 540 at
//!   `v = 1000`. The comparison is exact: `5a <= 4900 - 7(v - 450)` for
//!   `v < 650`, and `35a <= 24500 - 16(v - 650)` below that.
//! * Eye sockets (holes): ellipses centered at `(+-380, 540)` with radii
//!   `(250, 120)`.
//! * Nasal opening (hole): triangle with its apex at `(0, 650)` and a base at
//!   `v = 790` of half width 90.
//! * Teeth (`v >= 830`): vertical gaps (holes) for `a <= 45` and for
//!   `285 <= a <= 375`, plus a horizontal gap for `905 <= v <= 940` and
//!   `a <= 480`. Together they leave four upper and four lower teeth.
//!
//! The internal gray mask is 255 inside the skull and 0 outside and in holes.
//!
//! # Blend
//!
//! With `mask` in `0..=255` and `opacity` in `0..=128`:
//!
//! ```text
//! alpha   = (mask * opacity + 127) / 255
//! channel = (input * (255 - alpha) + bone * alpha + 127) / 255
//! ```
//!
//! where bone is `[229, 223, 209]`. Output alpha is always 255. For mask 255,
//! opacity 128 and input `[10, 20, 30]` the result is `[120, 122, 120]`.

use std::fmt;

const MAX_WIDTH: usize = 640;
const MAX_HEIGHT: usize = 480;
const MAX_OPACITY: u8 = 128;
const BONE: [u32; 3] = [229, 223, 209];

/// Signed top-left corner and size of the skull rectangle, in frame pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Skull placement and strength. `opacity` is 0 (off) to 128 (maximum).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkullOptions {
    pub bounds: OverlayRect,
    pub opacity: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkullError {
    EmptyDimension,
    DimensionsOverflow,
    DimensionsTooLarge,
    InputLength,
    OutputTooShort,
    InvalidBounds,
    OpacityOutOfRange,
}

impl fmt::Display for SkullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyDimension => "frame dimensions must be nonzero",
            Self::DimensionsOverflow => "frame dimensions overflow",
            Self::DimensionsTooLarge => "frame dimensions must not exceed 640 x 480",
            Self::InputLength => "input length must be exactly width * height * 4",
            Self::OutputTooShort => "output buffer is too short",
            Self::InvalidBounds => "skull bounds must be 1..=640 wide and 1..=480 high",
            Self::OpacityOutOfRange => "opacity must not exceed 128",
        };
        f.write_str(message)
    }
}

impl std::error::Error for SkullError {}

/// Returns the skull mask (0 or 255) at rectangle-local pixel `(i, j)` of a
/// `w x h` rectangle. Requires `0 <= i < w` and `0 <= j < h`.
fn skull_mask(i: i64, j: i64, w: i64, h: i64) -> u8 {
    let u = (2 * i + 1 - w) * 1000 / w;
    let v = (2 * j + 1) * 500 / h;
    let a = u.abs();

    let inside = if v < 450 {
        a * a * 450 * 450 + (v - 450) * (v - 450) * 980 * 980 <= 980 * 980 * 450 * 450
    } else if v < 650 {
        a * 5 <= 980 * 5 - (v - 450) * 7
    } else {
        a * 35 <= 700 * 35 - (v - 650) * 16
    };
    if !inside {
        return 0;
    }

    let eye = (a - 380) * (a - 380) * 120 * 120 + (v - 540) * (v - 540) * 250 * 250
        <= 250 * 250 * 120 * 120;
    let nose = (650..=790).contains(&v) && a * 140 <= (v - 650) * 90;
    let tooth_gap = v >= 830 && (a <= 45 || (285..=375).contains(&a));
    let mouth_gap = (905..=940).contains(&v) && a <= 480;
    if eye || nose || tooth_gap || mouth_gap {
        0
    } else {
        255
    }
}

fn blend(input: u8, alpha: u32, bone: u32) -> u8 {
    ((u32::from(input) * (255 - alpha) + bone * alpha + 127) / 255) as u8
}

/// Copies packed RGBA8 `input` to `output` with alpha 255, then blends the
/// bone-white skull over the part of `options.bounds` inside the frame.
///
/// Arguments are validated in the order of the `SkullError` variants and the
/// output is untouched on error. Output beyond `width * height * 4` bytes is
/// preserved. Work is bounded by the frame and the clipped rectangle; nothing
/// is allocated or retained.
pub fn render_skull_rgba(
    input: &[u8],
    output: &mut [u8],
    width: usize,
    height: usize,
    options: SkullOptions,
) -> Result<(), SkullError> {
    if width == 0 || height == 0 {
        return Err(SkullError::EmptyDimension);
    }
    let pixels = width
        .checked_mul(height)
        .ok_or(SkullError::DimensionsOverflow)?;
    let bytes = pixels
        .checked_mul(4)
        .ok_or(SkullError::DimensionsOverflow)?;
    if width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(SkullError::DimensionsTooLarge);
    }
    if input.len() != bytes {
        return Err(SkullError::InputLength);
    }
    if output.len() < bytes {
        return Err(SkullError::OutputTooShort);
    }
    let rect = options.bounds;
    if rect.width == 0
        || rect.width as usize > MAX_WIDTH
        || rect.height == 0
        || rect.height as usize > MAX_HEIGHT
    {
        return Err(SkullError::InvalidBounds);
    }
    if options.opacity > MAX_OPACITY {
        return Err(SkullError::OpacityOutOfRange);
    }

    let (out_pixels, _) = output[..bytes].as_chunks_mut::<4>();
    let (in_pixels, _) = input.as_chunks::<4>();
    for (dst, src) in out_pixels.iter_mut().zip(in_pixels) {
        dst[..3].copy_from_slice(&src[..3]);
        dst[3] = 255;
    }
    if options.opacity == 0 {
        return Ok(());
    }

    let (rx, ry) = (i64::from(rect.x), i64::from(rect.y));
    let (rw, rh) = (i64::from(rect.width), i64::from(rect.height));
    let x0 = rx.max(0);
    let x1 = (rx + rw).min(width as i64);
    let y0 = ry.max(0);
    let y1 = (ry + rh).min(height as i64);
    if x0 >= x1 || y0 >= y1 {
        return Ok(());
    }

    let opacity = u32::from(options.opacity);
    for py in y0..y1 {
        for px in x0..x1 {
            let mask = u32::from(skull_mask(px - rx, py - ry, rw, rh));
            if mask == 0 {
                continue;
            }
            let alpha = (mask * opacity + 127) / 255;
            let offset = (py as usize * width + px as usize) * 4;
            for (channel, bone) in output[offset..offset + 3].iter_mut().zip(BONE) {
                *channel = blend(*channel, alpha, bone);
            }
        }
    }
    Ok(())
}
