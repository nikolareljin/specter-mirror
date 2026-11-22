//! Sobel edge magnitude (L1 norm) over grayscale rows.
use crate::FrameError;

/// Writes `min(|Gx| + |Gy|, 255)` for interior pixels into tightly packed
/// output. Border pixels are zero. Input padding is ignored and output bytes
/// past `width * height` are left unchanged. Errors leave output unchanged.
pub fn sobel_l1(
    input: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    output: &mut [u8],
) -> Result<(), FrameError> {
    if width == 0 || height == 0 {
        return Err(FrameError::EmptyDimension);
    }
    let count = width
        .checked_mul(height)
        .ok_or(FrameError::DimensionsOverflow)?;
    let required = (height - 1)
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(width))
        .ok_or(FrameError::DimensionsOverflow)?;
    if stride < width {
        return Err(FrameError::StrideTooSmall);
    }
    if input.len() < required {
        return Err(FrameError::BufferTooShort);
    }
    let output = output.get_mut(..count).ok_or(FrameError::OutputTooShort)?;
    output.fill(0);
    if width < 3 || height < 3 {
        return Ok(());
    }
    for y in 1..height - 1 {
        let top = &input[(y - 1) * stride..][..width];
        let mid = &input[y * stride..][..width];
        let bot = &input[(y + 1) * stride..][..width];
        let out_row = &mut output[y * width..][..width];
        for x in 1..width - 1 {
            let p = |row: &[u8], dx: usize| i32::from(row[x + dx - 1]);
            let gx =
                (p(top, 2) - p(top, 0)) + 2 * (p(mid, 2) - p(mid, 0)) + (p(bot, 2) - p(bot, 0));
            let gy =
                (p(bot, 0) - p(top, 0)) + 2 * (p(bot, 1) - p(top, 1)) + (p(bot, 2) - p(top, 2));
            out_row[x] = (gx.abs() + gy.abs()).min(255) as u8;
        }
    }
    Ok(())
}
