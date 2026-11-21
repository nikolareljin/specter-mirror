//! RGB24 to 8-bit grayscale conversion using integer weights.
use crate::{FrameError, RgbFrame};

/// Writes tightly packed grayscale rows in the original orientation.
///
/// Uses `(77 * r + 150 * g + 29 * b + 128) >> 8`. Short output is rejected
/// before writing, and output bytes past `width * height` are left unchanged.
pub fn grayscale_rgb24(frame: &RgbFrame<'_>, output: &mut [u8]) -> Result<(), FrameError> {
    let width = frame.width();
    let count = width
        .checked_mul(frame.height())
        .ok_or(FrameError::DimensionsOverflow)?;
    let output = output.get_mut(..count).ok_or(FrameError::OutputTooShort)?;
    for (out_row, in_row) in output
        .chunks_exact_mut(width)
        .zip(frame.pixels().chunks(frame.stride()))
    {
        let (rgb_pixels, _) = in_row.as_chunks::<3>();
        for (out, rgb) in out_row.iter_mut().zip(rgb_pixels) {
            let r = u32::from(rgb[0]);
            let g = u32::from(rgb[1]);
            let b = u32::from(rgb[2]);
            *out = ((77 * r + 150 * g + 29 * b + 128) >> 8) as u8;
        }
    }
    Ok(())
}
