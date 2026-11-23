//! Changed-pixel observation between two grayscale8 frames.
//!
//! This measures how many pixels changed by more than a threshold. It does not
//! establish a person, face, identity or departure. Uniform lighting changes
//! (exposure, auto-gain, flicker, a screen turning on) change many pixels and
//! can therefore report high activity.

use crate::FrameError;
use std::fmt;

const MAX_WIDTH: usize = 640;
const MAX_HEIGHT: usize = 480;
/// Coordinate reported for an axis that is one pixel wide or tall.
const SINGLE_AXIS: u16 = 32768;

/// A position normalized to `0..=65535` on each axis, in source coordinates.
/// No mirroring or scaling is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalizedPoint {
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionObservation {
    /// `floor(changed_pixels * 1000 / pixel_count)`, in `0..=1000`.
    pub activity: u16,
    pub changed_pixels: usize,
    /// `None` only when no pixel changed, even if `activity` rounds to zero.
    pub centroid: Option<NormalizedPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionError {
    Frame(FrameError),
    DimensionsTooLarge,
}

impl fmt::Display for MotionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(error) => error.fmt(f),
            Self::DimensionsTooLarge => f.write_str("frame dimensions must not exceed 640 x 480"),
        }
    }
}

impl std::error::Error for MotionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Frame(error) => Some(error),
            Self::DimensionsTooLarge => None,
        }
    }
}

impl From<FrameError> for MotionError {
    fn from(error: FrameError) -> Self {
        Self::Frame(error)
    }
}

/// Compares two grayscale8 frames of identical geometry. A pixel changed when
/// `|current - previous| > pixel_threshold` (strictly greater). Row padding and
/// trailing bytes are ignored; padding after the last row is optional.
/// Allocates nothing and keeps no state.
///
/// Validation order: nonzero dimensions, checked `width * height` and
/// `(height - 1) * stride + width`, size limits (640 x 480), `stride >= width`,
/// then the length of `current` and of `previous`.
pub fn motion_gray8(
    current: &[u8],
    previous: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    pixel_threshold: u8,
) -> Result<MotionObservation, MotionError> {
    if width == 0 || height == 0 {
        return Err(FrameError::EmptyDimension.into());
    }
    let count = width
        .checked_mul(height)
        .ok_or(FrameError::DimensionsOverflow)?;
    let required = (height - 1)
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(width))
        .ok_or(FrameError::DimensionsOverflow)?;
    if width > MAX_WIDTH || height > MAX_HEIGHT {
        return Err(MotionError::DimensionsTooLarge);
    }
    if stride < width {
        return Err(FrameError::StrideTooSmall.into());
    }
    if current.len() < required || previous.len() < required {
        return Err(FrameError::BufferTooShort.into());
    }

    let (mut changed, mut sum_x, mut sum_y) = (0u64, 0u64, 0u64);
    for y in 0..height {
        let now = &current[y * stride..][..width];
        let before = &previous[y * stride..][..width];
        let mut row_changed = 0u64;
        for (x, (&a, &b)) in now.iter().zip(before).enumerate() {
            if a.abs_diff(b) > pixel_threshold {
                row_changed += 1;
                sum_x += x as u64;
            }
        }
        changed += row_changed;
        sum_y += row_changed * y as u64;
    }
    if changed == 0 {
        return Ok(MotionObservation {
            activity: 0,
            changed_pixels: 0,
            centroid: None,
        });
    }
    let axis = |sum: u64, extent: usize| -> u16 {
        if extent == 1 {
            SINGLE_AXIS
        } else {
            (sum * 65535 / (changed * (extent as u64 - 1))) as u16
        }
    };
    Ok(MotionObservation {
        activity: (changed * 1000 / count as u64) as u16,
        changed_pixels: changed as usize,
        centroid: Some(NormalizedPoint {
            x: axis(sum_x, width),
            y: axis(sum_y, height),
        }),
    })
}
