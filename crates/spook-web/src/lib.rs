//! WASM target compatibility proof for the shared frame engine.

use spook_core::{copy_mirrored_rgb24, FrameError, RgbFrame};

pub fn synthetic_frame_proof() -> Result<[u32; 2], FrameError> {
    let bytes = [255, 0, 0, 0, 255, 0];
    let frame = RgbFrame::new(2, 1, 6, &bytes)?;
    let mut output = [0; 2];
    copy_mirrored_rgb24(&frame, &mut output)?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    #[test]
    fn shared_frame_path_returns_mirrored_pixels() {
        assert_eq!(
            super::synthetic_frame_proof().unwrap(),
            [0x00ff00, 0xff0000]
        );
    }
}
