//! Portable borrowed-frame validation and a synthetic mirror proof.

use std::fmt;

mod director;
mod dither;
mod grayscale;
mod haunt;
mod motion;
mod progression;
mod skull;
mod sobel;
mod temporal;

pub use director::{
    DirectorConfig, DirectorError, DirectorInput, DirectorSnapshot, HauntDirector, ECHO, LAG, SKULL,
};
pub use dither::bayer_dither_gray8;
pub use grayscale::grayscale_rgb24;
pub use haunt::{HauntConfig, HauntError, HauntMachine, HauntSnapshot, HauntState};
pub use motion::{motion_gray8, MotionError, MotionObservation, NormalizedPoint};
pub use progression::{
    HauntLevel, HauntProgression, HauntTiming, HAUNT_FULL_STRENGTH, MAX_STAGE_MS, MIN_STAGE_MS,
};
pub use skull::{render_skull_rgba, OverlayRect, SkullError, SkullOptions};
pub use sobel::sobel_l1;
pub use temporal::{TemporalError, TemporalHistory, TemporalOptions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    EmptyDimension,
    DimensionsOverflow,
    StrideTooSmall,
    BufferTooShort,
    OutputTooShort,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyDimension => "frame dimensions must be nonzero",
            Self::DimensionsOverflow => "frame dimensions overflow",
            Self::StrideTooSmall => "stride is smaller than one pixel row",
            Self::BufferTooShort => "frame buffer is too short",
            Self::OutputTooShort => "output buffer is too short",
        };
        f.write_str(message)
    }
}

impl std::error::Error for FrameError {}

/// RGB24 rows with byte stride; padding after the final row is optional.
#[derive(Clone, Copy, Debug)]
pub struct RgbFrame<'a> {
    width: usize,
    height: usize,
    stride: usize,
    pixels: &'a [u8],
}

impl<'a> RgbFrame<'a> {
    pub fn new(
        width: usize,
        height: usize,
        stride: usize,
        pixels: &'a [u8],
    ) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::EmptyDimension);
        }
        let row_bytes = width.checked_mul(3).ok_or(FrameError::DimensionsOverflow)?;
        if stride < row_bytes {
            return Err(FrameError::StrideTooSmall);
        }
        let required = (height - 1)
            .checked_mul(stride)
            .and_then(|offset| offset.checked_add(row_bytes))
            .ok_or(FrameError::DimensionsOverflow)?;
        width
            .checked_mul(height)
            .ok_or(FrameError::DimensionsOverflow)?;
        if pixels.len() < required {
            return Err(FrameError::BufferTooShort);
        }
        Ok(Self {
            width,
            height,
            stride,
            pixels,
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn stride(&self) -> usize {
        self.stride
    }

    pub fn pixels(&self) -> &'a [u8] {
        self.pixels
    }
}

/// Writes mirrored 0x00RRGGBB pixels without allocating or changing trailing output.
pub fn copy_mirrored_rgb24(frame: &RgbFrame<'_>, output: &mut [u32]) -> Result<(), FrameError> {
    let count = frame.width * frame.height;
    if output.len() < count {
        return Err(FrameError::OutputTooShort);
    }
    for y in 0..frame.height {
        for x in 0..frame.width {
            let input = y * frame.stride + (frame.width - 1 - x) * 3;
            output[y * frame.width + x] = (u32::from(frame.pixels[input]) << 16)
                | (u32::from(frame.pixels[input + 1]) << 8)
                | u32::from(frame.pixels[input + 2]);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_and_stride_are_validated() {
        assert_eq!(
            RgbFrame::new(0, 1, 3, &[]).unwrap_err(),
            FrameError::EmptyDimension
        );
        assert_eq!(
            RgbFrame::new(1, 0, 3, &[]).unwrap_err(),
            FrameError::EmptyDimension
        );
        assert_eq!(
            RgbFrame::new(1, 1, 2, &[0; 3]).unwrap_err(),
            FrameError::StrideTooSmall
        );
        assert_eq!(
            RgbFrame::new(2, 1, 6, &[0; 5]).unwrap_err(),
            FrameError::BufferTooShort
        );
    }

    #[test]
    fn arithmetic_overflow_returns_an_error() {
        assert_eq!(
            RgbFrame::new(usize::MAX, 1, usize::MAX, &[]).unwrap_err(),
            FrameError::DimensionsOverflow,
        );
        assert_eq!(
            RgbFrame::new(1, usize::MAX, 4, &[]).unwrap_err(),
            FrameError::DimensionsOverflow,
        );
    }

    #[test]
    fn padded_rows_are_mirrored_without_reading_padding() {
        let input = [255, 0, 0, 0, 255, 0, 9, 9, 0, 0, 255, 255, 255, 255];
        let frame = RgbFrame::new(2, 2, 8, &input).unwrap();
        let mut output = [0; 5];
        output[4] = 0x1234;
        copy_mirrored_rgb24(&frame, &mut output).unwrap();
        assert_eq!(output, [0x00ff00, 0xff0000, 0xffffff, 0x0000ff, 0x1234]);
    }

    #[test]
    fn short_output_is_rejected_before_writing() {
        let frame = RgbFrame::new(2, 1, 6, &[0; 6]).unwrap();
        let mut output = [0x1234];
        assert_eq!(
            copy_mirrored_rgb24(&frame, &mut output),
            Err(FrameError::OutputTooShort)
        );
        assert_eq!(output, [0x1234]);
    }
}
