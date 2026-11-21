//! Deterministic 4 x 4 ordered Bayer dithering for 8-bit grayscale.
use crate::FrameError;

const BAYER_4X4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Writes tightly packed binary pixels (0 or 255) in the original orientation.
///
/// The matrix is anchored at image coordinate (0, 0). A pixel is 255 when its
/// value is at least `16 * matrix[y % 4][x % 4] + 8`. Input uses byte `stride`
/// and may omit final-row padding. Errors are returned before any output write,
/// and output bytes past `width * height` are left unchanged.
pub fn bayer_dither_gray8(
    input: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    output: &mut [u8],
) -> Result<(), FrameError> {
    if width == 0 || height == 0 {
        return Err(FrameError::EmptyDimension);
    }
    let required_input = (height - 1)
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(width))
        .ok_or(FrameError::DimensionsOverflow)?;
    let required_output = width
        .checked_mul(height)
        .ok_or(FrameError::DimensionsOverflow)?;
    if stride < width {
        return Err(FrameError::StrideTooSmall);
    }
    if input.len() < required_input {
        return Err(FrameError::BufferTooShort);
    }
    let output = output
        .get_mut(..required_output)
        .ok_or(FrameError::OutputTooShort)?;
    for (y, out_row) in output.chunks_exact_mut(width).enumerate() {
        let in_row = &input[y * stride..y * stride + width];
        let matrix_row = &BAYER_4X4[y % 4];
        for (x, (out, &value)) in out_row.iter_mut().zip(in_row).enumerate() {
            let threshold = 16 * matrix_row[x % 4] + 8;
            *out = if value >= threshold { 255 } else { 0 };
        }
    }
    Ok(())
}
