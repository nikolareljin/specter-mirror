use spook_core::{bayer_dither_gray8, grayscale_rgb24, sobel_l1, FrameError, RgbFrame};

const MATRIX: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const FIXTURE: &str = include_str!("fixtures/bayer_ramp.pgm");

fn threshold(x: usize, y: usize) -> u8 {
    16 * MATRIX[y % 4][x % 4] + 8
}

fn expected(value: u8, x: usize, y: usize) -> u8 {
    if value >= threshold(x, y) {
        255
    } else {
        0
    }
}

fn parse_fixture() -> Vec<u8> {
    assert!(FIXTURE.starts_with("P2\n"));
    let text: String = FIXTURE
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let mut tokens = text.split_whitespace();
    assert_eq!(tokens.next(), Some("P2"));
    assert_eq!(tokens.next(), Some("8"));
    assert_eq!(tokens.next(), Some("4"));
    assert_eq!(tokens.next(), Some("255"));
    let pixels: Vec<u8> = tokens
        .by_ref()
        .take(32)
        .map(|t| t.parse().unwrap())
        .collect();
    assert_eq!(pixels.len(), 32);
    assert_eq!(tokens.next(), None);
    pixels
}

#[test]
fn golden_fixture_matches() {
    let input: Vec<u8> = (0..32).map(|i| ((i * 16) % 256) as u8).collect();
    let want = parse_fixture();
    let mut out = [7u8; 33];
    bayer_dither_gray8(&input, 8, 4, 8, &mut out).unwrap();
    assert_eq!(&out[..32], &want[..]);
    assert_eq!(out[32], 7);
}

#[test]
fn black_and_white_are_preserved() {
    let mut out = [9u8; 20];
    bayer_dither_gray8(&[0; 20], 5, 4, 5, &mut out).unwrap();
    assert_eq!(out, [0; 20]);
    bayer_dither_gray8(&[255; 20], 5, 4, 5, &mut out).unwrap();
    assert_eq!(out, [255; 20]);
}

#[test]
fn every_matrix_position_has_exact_threshold() {
    for y in 0..4 {
        for x in 0..4 {
            let t = threshold(x, y);
            let mut input = [0u8; 16];
            input[y * 4 + x] = t;
            let mut out = [5u8; 16];
            bayer_dither_gray8(&input, 4, 4, 4, &mut out).unwrap();
            assert_eq!(out[y * 4 + x], 255, "equality at ({x},{y})");
            input[y * 4 + x] = t - 1;
            bayer_dither_gray8(&input, 4, 4, 4, &mut out).unwrap();
            assert_eq!(out[y * 4 + x], 0, "one below at ({x},{y})");
        }
    }
}

#[test]
fn block_of_128_matches_matrix_thresholds() {
    let mut out = [5u8; 16];
    bayer_dither_gray8(&[128; 16], 4, 4, 4, &mut out).unwrap();
    // 128 >= 16 * m + 8 exactly when m <= 7.
    let want = [
        255, 0, 255, 0, //
        0, 255, 0, 255, //
        255, 0, 255, 0, //
        0, 255, 0, 255,
    ];
    assert_eq!(out, want);
}

#[test]
fn pattern_repeats_for_odd_and_large_sizes() {
    for &(w, h) in &[(5usize, 7usize), (9, 6), (13, 11), (4, 9)] {
        let input: Vec<u8> = (0..w * h).map(|i| (i * 37 % 256) as u8).collect();
        let mut out = vec![3u8; w * h];
        bayer_dither_gray8(&input, w, h, w, &mut out).unwrap();
        for y in 0..h {
            for x in 0..w {
                assert_eq!(out[y * w + x], expected(input[y * w + x], x, y), "{w}x{h}");
            }
        }
    }
}

#[test]
fn padded_input_matches_packed_input() {
    let (w, h, stride) = (6usize, 5usize, 9usize);
    let packed: Vec<u8> = (0..w * h).map(|i| (i * 29 % 256) as u8).collect();
    let mut padded = vec![255u8; (h - 1) * stride + w];
    for y in 0..h {
        padded[y * stride..y * stride + w].copy_from_slice(&packed[y * w..(y + 1) * w]);
    }
    let mut want = vec![0u8; w * h];
    bayer_dither_gray8(&packed, w, h, w, &mut want).unwrap();
    let mut got = vec![1u8; w * h + 2];
    bayer_dither_gray8(&padded, w, h, stride, &mut got).unwrap();
    assert_eq!(&got[..w * h], &want[..]);
    assert_eq!(&got[w * h..], &[1, 1]);
    // With the final row's padding present too.
    padded.extend_from_slice(&[0; 3]);
    let mut again = vec![1u8; w * h];
    bayer_dither_gray8(&padded, w, h, stride, &mut again).unwrap();
    assert_eq!(again, want);
}

#[test]
fn repeated_calls_do_not_depend_on_previous_state() {
    let input = [200u8; 12];
    let mut a = [0u8; 12];
    let mut b = [0u8; 12];
    bayer_dither_gray8(&input, 4, 3, 4, &mut a).unwrap();
    bayer_dither_gray8(&[0; 5], 5, 1, 5, &mut [0; 5]).unwrap();
    bayer_dither_gray8(&input, 4, 3, 4, &mut b).unwrap();
    assert_eq!(a, b);
}

#[test]
fn tiny_frames_work() {
    let mut out = [9u8; 3];
    bayer_dither_gray8(&[8], 1, 1, 1, &mut out).unwrap();
    assert_eq!(out, [255, 9, 9]);
    bayer_dither_gray8(&[7], 1, 1, 1, &mut out).unwrap();
    assert_eq!(out, [0, 9, 9]);

    // 1 x N: column 0, thresholds 8, 200, 56, 248, 8.
    let col = [8, 199, 56, 247, 7];
    let mut out = [9u8; 6];
    bayer_dither_gray8(&col, 1, 5, 1, &mut out).unwrap();
    assert_eq!(out, [255, 0, 255, 0, 0, 9]);

    // N x 1: row 0, thresholds 8, 136, 40, 168, 8.
    let row = [8, 135, 40, 167, 8];
    let mut out = [9u8; 6];
    bayer_dither_gray8(&row, 5, 1, 5, &mut out).unwrap();
    assert_eq!(out, [255, 0, 255, 0, 255, 9]);
}

#[test]
fn errors_leave_output_unchanged() {
    let mut out = [5u8; 9];
    assert_eq!(
        bayer_dither_gray8(&[0; 9], 0, 3, 3, &mut out),
        Err(FrameError::EmptyDimension)
    );
    assert_eq!(out, [5; 9]);
    assert_eq!(
        bayer_dither_gray8(&[0; 9], 3, 0, 3, &mut out),
        Err(FrameError::EmptyDimension)
    );
    assert_eq!(out, [5; 9]);
    assert_eq!(
        bayer_dither_gray8(&[0; 9], 3, 3, 2, &mut out),
        Err(FrameError::StrideTooSmall)
    );
    assert_eq!(out, [5; 9]);
    assert_eq!(
        bayer_dither_gray8(&[0; 8], 3, 3, 3, &mut out),
        Err(FrameError::BufferTooShort)
    );
    assert_eq!(out, [5; 9]);
    let mut short = [5u8; 8];
    assert_eq!(
        bayer_dither_gray8(&[0; 9], 3, 3, 3, &mut short),
        Err(FrameError::OutputTooShort)
    );
    assert_eq!(short, [5; 8]);
    assert_eq!(
        bayer_dither_gray8(&[], usize::MAX, 2, usize::MAX, &mut out),
        Err(FrameError::DimensionsOverflow)
    );
    assert_eq!(out, [5; 9]);
    assert_eq!(
        bayer_dither_gray8(&[], 1, usize::MAX, 2, &mut out),
        Err(FrameError::DimensionsOverflow)
    );
    assert_eq!(out, [5; 9]);
    assert_eq!(
        bayer_dither_gray8(&[], usize::MAX, usize::MAX, usize::MAX, &mut out),
        Err(FrameError::DimensionsOverflow)
    );
    assert_eq!(out, [5; 9]);
    assert_eq!(
        bayer_dither_gray8(&[], 2, 2, usize::MAX, &mut out),
        Err(FrameError::DimensionsOverflow)
    );
    assert_eq!(out, [5; 9]);
}

#[test]
fn grayscale_sobel_and_dither_reuse_buffers() {
    const SENTINEL: u8 = 0xA5;
    let (w, h, stride) = (6usize, 5usize, 20usize);
    let mut rgb = vec![0u8; (h - 1) * stride + w * 3];
    for y in 0..h {
        for x in 0..w {
            let o = y * stride + x * 3;
            let v = (x * 40 + y * 10) as u8;
            rgb[o..o + 3].copy_from_slice(&[v, v / 2, 255 - v]);
        }
    }
    let n = w * h;
    let mut gray = vec![SENTINEL; n + 2];
    let mut edges = vec![SENTINEL; n + 2];
    let mut dither = vec![SENTINEL; n + 2];
    let before = [
        (gray.as_ptr(), gray.capacity()),
        (edges.as_ptr(), edges.capacity()),
        (dither.as_ptr(), dither.capacity()),
    ];
    let mut first: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = None;
    for _ in 0..4 {
        let frame = RgbFrame::new(w, h, stride, &rgb).unwrap();
        grayscale_rgb24(&frame, &mut gray).unwrap();
        sobel_l1(&gray[..n], w, h, w, &mut edges).unwrap();
        bayer_dither_gray8(&gray[..n], w, h, w, &mut dither).unwrap();
        for buf in [&gray, &edges, &dither] {
            assert_eq!(&buf[n..], &[SENTINEL; 2]);
        }
        assert!(dither[..n].iter().all(|&v| v == 0 || v == 255));
        let now = (gray.clone(), edges.clone(), dither.clone());
        match &first {
            None => first = Some(now),
            Some(f) => assert_eq!(*f, now),
        }
    }
    let after = [
        (gray.as_ptr(), gray.capacity()),
        (edges.as_ptr(), edges.capacity()),
        (dither.as_ptr(), dither.capacity()),
    ];
    assert_eq!(before, after);
}
