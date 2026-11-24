//! Literal motion cases. Expected numbers were worked out independently from
//! the formulas in the task, not read back from the implementation.

use spook_core::{motion_gray8, FrameError, MotionError, MotionObservation, NormalizedPoint};

fn obs(activity: u16, changed: usize, x: u16, y: u16) -> MotionObservation {
    MotionObservation {
        activity,
        changed_pixels: changed,
        centroid: Some(NormalizedPoint { x, y }),
    }
}

const NONE: MotionObservation = MotionObservation {
    activity: 0,
    changed_pixels: 0,
    centroid: None,
};

/// Two tightly packed frames where `points` differ by `delta`.
fn pair(w: usize, h: usize, points: &[(usize, usize)], delta: u8) -> (Vec<u8>, Vec<u8>) {
    let previous = vec![100; w * h];
    let mut current = previous.clone();
    for &(x, y) in points {
        current[y * w + x] = 100 + delta;
    }
    (current, previous)
}

fn block(x0: usize, x1: usize, y0: usize, y1: usize) -> Vec<(usize, usize)> {
    (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .collect()
}

fn check(w: usize, h: usize, points: &[(usize, usize)]) -> MotionObservation {
    let (c, p) = pair(w, h, points, 50);
    motion_gray8(&c, &p, w, h, w, 10).unwrap()
}

#[test]
fn unchanged_frames_have_no_activity_and_no_centroid() {
    let frame = vec![77; 12];
    assert_eq!(motion_gray8(&frame, &frame, 4, 3, 4, 0).unwrap(), NONE);
    assert_eq!(check(8, 8, &[]), NONE);
}

#[test]
fn threshold_comparison_is_strictly_greater() {
    // Differences of exactly 10, 11 and (in the other direction) 11.
    let previous = [100, 100, 100, 100];
    let current = [110, 111, 89, 90];
    // |diff| = 10, 11, 11, 10 with threshold 10: pixels 1 and 2 change.
    assert_eq!(
        motion_gray8(&current, &previous, 4, 1, 4, 10).unwrap(),
        obs(500, 2, 32767, 32768)
    );
    // Threshold equal to the difference: nothing changes.
    assert_eq!(
        motion_gray8(&current, &previous, 4, 1, 4, 11).unwrap(),
        NONE
    );
    // Threshold zero: any difference counts; equal pixels do not.
    let one = motion_gray8(&[5, 6], &[5, 5], 2, 1, 2, 0).unwrap();
    assert_eq!((one.changed_pixels, one.centroid.unwrap().x), (1, 65535));
    // Threshold 255 can never be exceeded.
    assert_eq!(
        motion_gray8(&[0, 255], &[255, 0], 2, 1, 2, 255).unwrap(),
        NONE
    );
    // Full swing still counts at 254.
    assert_eq!(
        motion_gray8(&[0, 255], &[255, 0], 2, 1, 2, 254)
            .unwrap()
            .changed_pixels,
        2
    );
}

#[test]
fn asymmetric_centroids_use_floor_normalization() {
    // 5 x 4, changed (0,0), (1,0), (4,3): x sum 5 / (3*4) * 65535, y sum 3 / (3*3).
    assert_eq!(
        check(5, 4, &[(0, 0), (1, 0), (4, 3)]),
        obs(150, 3, 27306, 21845)
    );
    // Corners reach the full range exactly.
    assert_eq!(check(640, 480, &[(0, 0)]), obs(0, 1, 0, 0));
    assert_eq!(check(640, 480, &[(639, 479)]), obs(0, 1, 65535, 65535));
    assert_eq!(check(640, 480, &[(320, 240)]), obs(0, 1, 32818, 32835));
}

#[test]
fn tiny_axes_use_the_single_pixel_coordinate() {
    assert_eq!(check(4, 1, &[(1, 0), (3, 0)]), obs(500, 2, 43690, 32768));
    assert_eq!(check(1, 3, &[(0, 2)]), obs(333, 1, 32768, 65535));
    assert_eq!(check(3, 1, &[(0, 0)]), obs(333, 1, 0, 32768));
    assert_eq!(check(1, 1, &[(0, 0)]), obs(1000, 1, 32768, 32768));
}

#[test]
fn padded_stride_ignores_padding_and_final_padding_is_optional() {
    // 3 x 2, stride 5: rows at 0 and 5; required length is 8.
    let mut previous = vec![100u8; 8];
    let mut current = previous.clone();
    current[0] = 200; // (0, 0)
    current[5 + 2] = 200; // (2, 1)
                          // Padding bytes (indices 3, 4) differ wildly and must not count.
    previous[3] = 0;
    current[3] = 255;
    previous[4] = 255;
    current[4] = 0;
    let expected = obs(333, 2, 32767, 32767);
    assert_eq!(
        motion_gray8(&current, &previous, 3, 2, 5, 10).unwrap(),
        expected
    );
    // With full final padding and sentinel trailing bytes, same answer.
    current.extend_from_slice(&[255, 255, 1]);
    previous.extend_from_slice(&[0, 0, 200]);
    assert_eq!(
        motion_gray8(&current, &previous, 3, 2, 5, 10).unwrap(),
        expected
    );
}

#[test]
fn trailing_bytes_beyond_the_frame_are_ignored() {
    let (mut c, mut p) = pair(4, 4, &[(2, 2)], 50);
    c.extend_from_slice(&[255; 9]);
    p.extend_from_slice(&[0; 9]);
    assert_eq!(
        motion_gray8(&c, &p, 4, 4, 4, 10).unwrap(),
        obs(62, 1, 43690, 43690)
    );
}

#[test]
fn processing_sizes_report_exact_literal_values() {
    // 160 x 120: 40 x 30 block at x 40..80, y 30..60 -> 1200 pixels.
    assert_eq!(
        check(160, 120, &block(40, 80, 30, 60)),
        obs(62, 1200, 24524, 24506)
    );
    // 320 x 240: 40 x 80 block at x 100..140, y 10..90 -> 3200 pixels.
    assert_eq!(
        check(320, 240, &block(100, 140, 10, 90)),
        obs(41, 3200, 24549, 13573)
    );
}

#[test]
fn the_640_by_480_limit_is_accepted() {
    let all = block(0, 640, 0, 480);
    assert_eq!(check(640, 480, &all), obs(1000, 307_200, 32767, 32767));
}

#[test]
fn centroid_exists_when_activity_rounds_to_zero() {
    let result = check(320, 240, &[(10, 20)]);
    assert_eq!((result.activity, result.changed_pixels), (0, 1));
    assert!(result.centroid.is_some());
}

#[test]
fn validation_errors_follow_the_specified_order() {
    let frame = [0u8; 16];
    let frame_err = |e| Err(MotionError::Frame(e));
    assert_eq!(
        motion_gray8(&frame, &frame, 0, 4, 4, 0),
        frame_err(FrameError::EmptyDimension)
    );
    assert_eq!(
        motion_gray8(&frame, &frame, 4, 0, 4, 0),
        frame_err(FrameError::EmptyDimension)
    );
    // Overflow is reported before the size limit and before stride checks.
    assert_eq!(
        motion_gray8(&[], &[], usize::MAX, 2, 1, 0),
        frame_err(FrameError::DimensionsOverflow)
    );
    assert_eq!(
        motion_gray8(&[], &[], 1, usize::MAX, 2, 0),
        frame_err(FrameError::DimensionsOverflow)
    );
    assert_eq!(
        motion_gray8(&[], &[], 2, 2, usize::MAX, 0),
        frame_err(FrameError::DimensionsOverflow)
    );
    // Height 1 makes both products fit, so the size limit is what fails.
    assert_eq!(
        motion_gray8(&[], &[], usize::MAX, 1, usize::MAX, 0),
        Err(MotionError::DimensionsTooLarge)
    );
    // Size limit before stride and buffers.
    assert_eq!(
        motion_gray8(&[], &[], 641, 1, 1, 0),
        Err(MotionError::DimensionsTooLarge)
    );
    assert_eq!(
        motion_gray8(&[], &[], 1, 481, 1, 0),
        Err(MotionError::DimensionsTooLarge)
    );
    assert_eq!(
        motion_gray8(&frame, &frame, 4, 4, 3, 0),
        frame_err(FrameError::StrideTooSmall)
    );
    // Required length is 3 * 6 + 4 = 22.
    let short = [0u8; 21];
    let exact = [0u8; 22];
    assert_eq!(
        motion_gray8(&short, &exact, 4, 4, 6, 0),
        frame_err(FrameError::BufferTooShort)
    );
    assert_eq!(
        motion_gray8(&exact, &short, 4, 4, 6, 0),
        frame_err(FrameError::BufferTooShort)
    );
    assert_eq!(motion_gray8(&exact, &exact, 4, 4, 6, 0), Ok(NONE));
}

#[test]
fn errors_display_and_chain() {
    let wrapped = MotionError::Frame(FrameError::BufferTooShort);
    assert_eq!(wrapped.to_string(), FrameError::BufferTooShort.to_string());
    assert!(std::error::Error::source(&wrapped).is_some());
    let big = MotionError::DimensionsTooLarge;
    assert_eq!(
        big.to_string(),
        "frame dimensions must not exceed 640 x 480"
    );
    assert!(std::error::Error::source(&big).is_none());
}

// Bounded seeded generated frames (xorshift64, no external crate) compared
// with a naive per-pixel reference that uses u128 and a different loop shape.
#[test]
fn generated_frames_match_a_naive_reference_and_stay_in_bounds() {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for case in 0..400 {
        let width = 1 + (next() % 24) as usize;
        let height = 1 + (next() % 24) as usize;
        let stride = width + (next() % 5) as usize;
        let threshold = (next() % 256) as u8;
        let len = (height - 1) * stride + width + (next() % 3) as usize;
        let sparse = next().is_multiple_of(2);
        let previous: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let current: Vec<u8> = previous
            .iter()
            .map(|&b| {
                if sparse && next() % 4 != 0 {
                    b
                } else {
                    next() as u8
                }
            })
            .collect();

        let (mut n, mut sx, mut sy) = (0u128, 0u128, 0u128);
        for i in 0..width * height {
            let (x, y) = (i % width, i / width);
            let (a, b) = (current[y * stride + x], previous[y * stride + x]);
            if (i16::from(a) - i16::from(b)).unsigned_abs() > u16::from(threshold) {
                n += 1;
                sx += x as u128;
                sy += y as u128;
            }
        }
        let result = motion_gray8(&current, &previous, width, height, stride, threshold)
            .unwrap_or_else(|e| panic!("case {case}: {e}"));
        assert!(result.activity <= 1000, "case {case}");
        assert_eq!(result.changed_pixels as u128, n, "case {case}");
        assert_eq!(
            u128::from(result.activity),
            n * 1000 / (width * height) as u128,
            "case {case}"
        );
        if n == 0 {
            assert_eq!(result.centroid, None);
        } else {
            let axis = |sum: u128, extent: usize| {
                if extent == 1 {
                    32768
                } else {
                    sum * 65535 / (n * (extent as u128 - 1))
                }
            };
            let c = result.centroid.expect("changed pixels need a centroid");
            assert_eq!(u128::from(c.x), axis(sx, width), "case {case}");
            assert_eq!(u128::from(c.y), axis(sy, height), "case {case}");
        }
    }
}
