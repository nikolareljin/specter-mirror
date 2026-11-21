use spook_core::{grayscale_rgb24, sobel_l1, FrameError, RgbFrame};

fn gray(w: usize, h: usize, stride: usize, px: &[u8], out: &mut [u8]) -> Result<(), FrameError> {
    grayscale_rgb24(&RgbFrame::new(w, h, stride, px)?, out)
}

#[test]
fn known_colors_use_exact_formula() {
    let px = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0];
    let mut out = [7; 6];
    gray(5, 1, 15, &px, &mut out).unwrap();
    assert_eq!(out, [77, 149, 29, 255, 0, 7]);
}

#[test]
fn neutral_grays_match_formula() {
    for v in 0..=255u8 {
        let mut out = [0];
        gray(1, 1, 3, &[v, v, v], &mut out).unwrap();
        let expected = ((256 * u32::from(v) + 128) >> 8) as u8;
        assert_eq!(out[0], expected);
    }
}

#[test]
fn grayscale_keeps_orientation_and_skips_padding() {
    // 2x2, stride 8, final padding omitted, padding bytes are 9.
    let px = [255, 0, 0, 0, 255, 0, 9, 9, 0, 0, 255, 255, 255, 255];
    let mut out = [1; 5];
    gray(2, 2, 8, &px, &mut out).unwrap();
    assert_eq!(out, [77, 149, 29, 255, 1]);
}

#[test]
fn grayscale_errors_do_not_mutate() {
    let mut out = [5; 3];
    assert_eq!(
        gray(2, 2, 6, &[0; 12], &mut out),
        Err(FrameError::OutputTooShort)
    );
    assert_eq!(
        gray(0, 1, 3, &[], &mut out),
        Err(FrameError::EmptyDimension)
    );
    assert_eq!(
        gray(1, 1, 2, &[0; 3], &mut out),
        Err(FrameError::StrideTooSmall)
    );
    assert_eq!(
        gray(2, 1, 6, &[0; 5], &mut out),
        Err(FrameError::BufferTooShort)
    );
    assert_eq!(
        gray(usize::MAX, 1, usize::MAX, &[], &mut out),
        Err(FrameError::DimensionsOverflow)
    );
    assert_eq!(out, [5; 3]);
}

#[test]
fn constant_input_has_no_edges() {
    let mut out = [9; 25];
    sobel_l1(&[100; 25], 5, 5, 5, &mut out).unwrap();
    assert_eq!(out, [0; 25]);
}

#[test]
fn vertical_step_gives_gx() {
    let input = [0, 0, 10, 0, 0, 10, 0, 0, 10];
    let mut out = [1; 9];
    sobel_l1(&input, 3, 3, 3, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 0, 40, 0, 0, 0, 0]);
    let reversed = [10, 0, 0, 10, 0, 0, 10, 0, 0];
    sobel_l1(&reversed, 3, 3, 3, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 0, 40, 0, 0, 0, 0]);
}

#[test]
fn horizontal_step_gives_gy() {
    let mut out = [1; 9];
    sobel_l1(&[0, 0, 0, 0, 0, 0, 10, 10, 10], 3, 3, 3, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 0, 40, 0, 0, 0, 0]);
    sobel_l1(&[10, 10, 10, 0, 0, 0, 0, 0, 0], 3, 3, 3, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 0, 40, 0, 0, 0, 0]);
}

#[test]
fn mixed_gradient_adds_both_magnitudes() {
    let mut out = [9; 10];
    sobel_l1(&[0, 1, 2, 3, 4, 5, 6, 7, 8], 3, 3, 3, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 0, 32, 0, 0, 0, 0, 9]);
}

#[test]
fn short_output_remains_unchanged() {
    let mut out = [5; 8];
    assert_eq!(
        sobel_l1(&[0; 9], 3, 3, 3, &mut out),
        Err(FrameError::OutputTooShort)
    );
    assert_eq!(out, [5; 8]);
}

#[test]
fn full_contrast_saturates_with_zero_borders() {
    let mut out = [9; 9];
    sobel_l1(&[0, 0, 255, 0, 0, 255, 0, 0, 255], 3, 3, 3, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 0, 255, 0, 0, 0, 0]);
}

#[test]
fn padded_input_ignores_padding_and_omits_final_padding() {
    // stride 5, width 3; padding bytes are 255 and must not matter.
    let input = [0, 0, 10, 255, 255, 0, 0, 10, 255, 255, 0, 0, 10];
    let mut out = [1; 10];
    sobel_l1(&input, 3, 3, 5, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 0, 40, 0, 0, 0, 0, 1]);
}

#[test]
fn tiny_frames_zero_output_and_preserve_trailing() {
    let mut out = [9; 7];
    sobel_l1(&[50; 6], 2, 3, 2, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 0, 0, 0, 9]);
    let mut out = [9; 4];
    sobel_l1(&[1, 200, 3], 3, 1, 3, &mut out).unwrap();
    assert_eq!(out, [0, 0, 0, 9]);
}

#[test]
fn sobel_errors_do_not_mutate() {
    let mut out = [5; 9];
    assert_eq!(
        sobel_l1(&[0; 9], 0, 3, 3, &mut out),
        Err(FrameError::EmptyDimension)
    );
    assert_eq!(
        sobel_l1(&[0; 9], 3, 0, 3, &mut out),
        Err(FrameError::EmptyDimension)
    );
    assert_eq!(
        sobel_l1(&[0; 9], 3, 3, 2, &mut out),
        Err(FrameError::StrideTooSmall)
    );
    assert_eq!(
        sobel_l1(&[0; 8], 3, 3, 3, &mut out),
        Err(FrameError::BufferTooShort)
    );
    assert_eq!(
        sobel_l1(&[0; 9], 3, 3, 3, &mut [5; 8]),
        Err(FrameError::OutputTooShort)
    );
    assert_eq!(
        sobel_l1(&[], usize::MAX, 2, usize::MAX, &mut out),
        Err(FrameError::DimensionsOverflow)
    );
    assert_eq!(
        sobel_l1(&[], 1, usize::MAX, 2, &mut out),
        Err(FrameError::DimensionsOverflow)
    );
    assert_eq!(
        sobel_l1(&[], usize::MAX, usize::MAX, usize::MAX, &mut out),
        Err(FrameError::DimensionsOverflow)
    );
    assert_eq!(out, [5; 9]);
}

#[test]
fn reused_buffers_are_deterministic() {
    let mut px = [0u8; 27];
    for (i, v) in px.iter_mut().enumerate() {
        *v = if (i / 3) % 2 == 0 { 0 } else { 255 };
    }
    let mut g = [0; 9];
    let mut e = [0; 9];
    let mut first = [0; 9];
    for i in 0..3 {
        gray(3, 3, 9, &px, &mut g).unwrap();
        sobel_l1(&g, 3, 3, 3, &mut e).unwrap();
        if i == 0 {
            first = e;
        }
        assert_eq!(e, first);
    }
}
