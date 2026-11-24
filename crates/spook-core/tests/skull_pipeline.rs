use spook_core::{render_skull_rgba, OverlayRect, SkullError, SkullOptions};

// The fixtures are plain PGM images of the gray mask inside a whole frame:
// 255 inside the skull, 0 outside and in holes. They were written by a
// separate script that follows the geometry documented in src/skull.rs, then
// reviewed as ASCII art. The 160 x 120 image holds a 48 x 64 rectangle at
// (56, 28); the 320 x 240 image holds a 96 x 128 rectangle at (112, 56).
const FIXTURE_160: &str = include_str!("fixtures/skull_160.pgm");
const FIXTURE_320: &str = include_str!("fixtures/skull_320.pgm");
const BONE: [u32; 3] = [229, 223, 209];

fn parse_pgm(text: &str, width: usize, height: usize) -> Vec<u8> {
    let body: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let mut tokens = body.split_whitespace();
    assert_eq!(tokens.next(), Some("P2"));
    assert_eq!(tokens.next(), Some(width.to_string().as_str()));
    assert_eq!(tokens.next(), Some(height.to_string().as_str()));
    assert_eq!(tokens.next(), Some("255"));
    let pixels: Vec<u8> = tokens.map(|token| token.parse().unwrap()).collect();
    assert_eq!(pixels.len(), width * height, "exactly one image");
    assert!(pixels.iter().all(|&p| p == 0 || p == 255));
    pixels
}

fn mask160() -> Vec<u8> {
    parse_pgm(FIXTURE_160, 160, 120)
}

fn mask320() -> Vec<u8> {
    parse_pgm(FIXTURE_320, 320, 240)
}

fn rect(x: i32, y: i32, width: u32, height: u32) -> OverlayRect {
    OverlayRect {
        x,
        y,
        width,
        height,
    }
}

fn options(bounds: OverlayRect, opacity: u8) -> SkullOptions {
    SkullOptions { bounds, opacity }
}

/// Asymmetric RGBA input with non-opaque alpha, so mirroring, transposition and
/// alpha pass-through are all detectable.
fn input_frame(width: usize, height: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            data.push(((x * 7 + y * 13 + 1) % 251) as u8);
            data.push(((x * 3 + y * 5 + 40) % 253) as u8);
            data.push(((x * 11 + y + 90) % 241) as u8);
            data.push(((x + 2 * y) % 200) as u8);
        }
    }
    data
}

/// Literal alpha and channel formulas from the specification.
fn expected_rgba(
    input: &[u8],
    width: usize,
    height: usize,
    mask_at: impl Fn(usize, usize) -> u8,
    opacity: u8,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for y in 0..height {
        for x in 0..width {
            let alpha = (u32::from(mask_at(x, y)) * u32::from(opacity) + 127) / 255;
            let base = (y * width + x) * 4;
            for c in 0..3 {
                let value =
                    (u32::from(input[base + c]) * (255 - alpha) + BONE[c] * alpha + 127) / 255;
                out.push(value as u8);
            }
            out.push(255);
        }
    }
    out
}

/// Mask of a rectangle whose local mask is a sub-window of a fixture image.
fn windowed_mask(
    fixture: &[u8],
    fixture_width: usize,
    origin: (usize, usize),
    size: (i64, i64),
    at: (i64, i64),
) -> impl Fn(usize, usize) -> u8 + '_ {
    move |x, y| {
        let i = x as i64 - at.0;
        let j = y as i64 - at.1;
        if i < 0 || j < 0 || i >= size.0 || j >= size.1 {
            0
        } else {
            fixture[(origin.1 + j as usize) * fixture_width + origin.0 + i as usize]
        }
    }
}

fn runs(values: impl Iterator<Item = u8>) -> Vec<(u8, usize)> {
    let mut result: Vec<(u8, usize)> = Vec::new();
    for value in values {
        match result.last_mut() {
            Some((last, count)) if *last == value => *count += 1,
            _ => result.push((value, 1)),
        }
    }
    result
}

fn render(
    input: &[u8],
    width: usize,
    height: usize,
    options: SkullOptions,
) -> Result<Vec<u8>, SkullError> {
    let mut output = vec![0xA5; input.len()];
    render_skull_rgba(input, &mut output, width, height, options)?;
    Ok(output)
}

// --- Golden masks ---------------------------------------------------------

#[test]
fn fixtures_are_single_original_pgm_images() {
    assert!(FIXTURE_160.starts_with("P2\n"));
    assert!(FIXTURE_320.starts_with("P2\n"));
    assert_eq!(mask160().len(), 160 * 120);
    assert_eq!(mask320().len(), 320 * 240);
    assert!(FIXTURE_160.contains("Original procedural"));
    assert!(FIXTURE_320.contains("Original procedural"));
}

#[test]
fn hand_checked_geometry_pixels_match_fixtures() {
    // Coordinates were worked out by hand from the documented formulas.
    let m = mask160();
    let at = |x: usize, y: usize| m[y * 160 + x];
    assert_eq!(at(56, 28), 0, "rectangle corner is outside the silhouette");
    assert_eq!(at(80, 36), 255, "forehead");
    assert_eq!(at(70, 62), 0, "left eye socket center");
    assert_eq!(at(80, 62), 255, "bridge between the sockets");
    assert_eq!(at(80, 75), 0, "nasal opening");
    assert_eq!(at(80, 83), 0, "center teeth gap");
    assert_eq!(at(71, 83), 0, "left teeth gap");
    assert_eq!(at(70, 83), 255, "tooth");

    let m = mask320();
    let at = |x: usize, y: usize| m[y * 320 + x];
    assert_eq!(at(160, 72), 255, "forehead");
    assert_eq!(at(140, 124), 0, "left eye socket");
    assert_eq!(at(160, 151), 0, "nasal opening");
    assert_eq!(at(160, 167), 0, "center teeth gap");
    assert_eq!(at(146, 167), 255, "tooth");
}

#[test]
fn masks_have_two_sockets_nose_and_teeth_at_both_sizes() {
    // (mask, frame width, x range of the rectangle, eye row, nose row, teeth row)
    let m160 = mask160();
    let m320 = mask320();
    let cases = [
        (&m160, 160, 56..104, 62, 75, 83),
        (&m320, 320, 112..208, 124, 151, 167),
    ];
    for (mask, width, xs, eye_y, nose_y, teeth_y) in cases {
        let row = |y: usize| runs(xs.clone().map(|x| mask[y * width + x]));
        let kinds = |r: &[(u8, usize)]| r.iter().map(|&(v, _)| v).collect::<Vec<_>>();

        let eyes = row(eye_y);
        assert_eq!(kinds(&eyes), [0, 255, 0, 255, 0, 255, 0], "row {eye_y}");
        assert!(eyes[2].1 >= 4 && eyes[4].1 >= 4, "sockets readable");
        assert!(eyes[3].1 >= 4, "sockets are separated");
        assert_eq!(eyes[2].1, eyes[4].1, "sockets are the same size");

        let nose = row(nose_y);
        assert_eq!(kinds(&nose), [0, 255, 0, 255, 0], "row {nose_y}");
        assert!(nose[2].1 >= 2);

        let teeth = row(teeth_y);
        assert_eq!(
            kinds(&teeth),
            [0, 255, 0, 255, 0, 255, 0, 255, 0],
            "four teeth, row {teeth_y}"
        );
    }

    // Narrower jaw: the lowest skull row is narrower than the widest row.
    let widest = (28..92)
        .map(|y| (56..104).filter(|&x| m160[y * 160 + x] != 0).count())
        .max()
        .unwrap();
    let chin = (56..104).filter(|&x| m160[91 * 160 + x] != 0).count();
    assert!(chin < widest, "jaw {chin} < cranium {widest}");
}

#[test]
fn renderer_reproduces_golden_masks() {
    // Black input at the maximum opacity: skull pixels become the literal
    // bone blend [115, 112, 105], all others stay black.
    let cases = [
        (160, 120, rect(56, 28, 48, 64), mask160()),
        (320, 240, rect(112, 56, 96, 128), mask320()),
    ];
    for (width, height, bounds, mask) in cases {
        let mut input = vec![0u8; width * height * 4];
        for pixel in input.as_chunks_mut::<4>().0.iter_mut() {
            pixel[3] = 255;
        }
        let output = render(&input, width, height, options(bounds, 128)).unwrap();
        for (index, pixel) in output.as_chunks::<4>().0.iter().enumerate() {
            let expected: [u8; 4] = if mask[index] == 255 {
                [115, 112, 105, 255]
            } else {
                [0, 0, 0, 255]
            };
            assert_eq!(*pixel, expected, "pixel {index} of {width}x{height}");
        }
    }
}

// --- Literal blends ---------------------------------------------------------

#[test]
fn literal_blend_values() {
    let input = [10, 20, 30, 77];
    let one = rect(0, 0, 1, 1);
    let at = |opacity| render(&input, 1, 1, options(one, opacity)).unwrap();
    assert_eq!(at(0), [10, 20, 30, 255]);
    assert_eq!(at(1), [11, 21, 31, 255]);
    assert_eq!(at(128), [120, 122, 120, 255]);
}

#[test]
fn opacity_extremes_on_full_frames() {
    let (w, h) = (160, 120);
    let input = input_frame(w, h);
    let bounds = rect(56, 28, 48, 64);
    let m = mask160();
    for opacity in [0u8, 1, 2, 64, 127, 128] {
        let output = render(&input, w, h, options(bounds, opacity)).unwrap();
        let expected = expected_rgba(&input, w, h, |x, y| m[y * w + x], opacity);
        assert_eq!(output, expected, "opacity {opacity}");
    }
    // Opacity 0 keeps every input RGB value and only forces alpha to 255.
    let off = render(&input, w, h, options(bounds, 0)).unwrap();
    for (o, i) in off
        .as_chunks::<4>()
        .0
        .iter()
        .zip(input.as_chunks::<4>().0.iter())
    {
        assert_eq!(o[..3], i[..3]);
        assert_eq!(o[3], 255);
    }
}

#[test]
fn orientation_is_single_and_frame_is_not_mirrored() {
    let (w, h) = (320, 240);
    let input = input_frame(w, h);
    let bounds = rect(112, 56, 96, 128);
    let m = mask320();
    let output = render(&input, w, h, options(bounds, 128)).unwrap();
    assert_eq!(
        output,
        expected_rgba(&input, w, h, |x, y| m[y * w + x], 128)
    );
    // Pixels outside the silhouette and in holes equal the input at the same
    // position, so nothing was mirrored, flipped or shifted.
    for y in 0..h {
        for x in 0..w {
            if m[y * w + x] == 0 {
                let i = (y * w + x) * 4;
                assert_eq!(output[i..i + 3], input[i..i + 3]);
            }
        }
    }
    // Skull pixels are brighter toward bone and differ from the input.
    assert_ne!(
        output[(72 * w + 160) * 4..][..3],
        input[(72 * w + 160) * 4..][..3]
    );
}

// --- Clipping ---------------------------------------------------------------

#[test]
fn clipping_crops_without_rescaling() {
    let (w, h) = (160usize, 120usize);
    let input = input_frame(w, h);
    let m = mask160();
    let positions: [(i32, i32); 14] = [
        (-20, -10),
        (-47, 0),
        (-48, 0),
        (-49, 30),
        (130, 100),
        (159, 0),
        (160, 0),
        (0, 119),
        (0, 120),
        (0, -63),
        (0, -64),
        (112, 56),
        (150, -30),
        (-30, 90),
    ];
    for (x, y) in positions {
        let output = render(&input, w, h, options(rect(x, y, 48, 64), 128)).unwrap();
        let mask = windowed_mask(&m, 160, (56, 28), (48, 64), (i64::from(x), i64::from(y)));
        let expected = expected_rgba(&input, w, h, mask, 128);
        assert_eq!(output, expected, "bounds at ({x}, {y})");
    }
}

#[test]
fn offscreen_and_extreme_positions_leave_rgb_unchanged() {
    let (w, h) = (40usize, 30usize);
    let input = input_frame(w, h);
    let untouched = expected_rgba(&input, w, h, |_, _| 0, 128);
    let extremes = [
        i32::MIN,
        i32::MIN + 1,
        -1_000_000,
        1_000_000,
        i32::MAX - 1,
        i32::MAX,
    ];
    for &x in &extremes {
        for &y in &[0, 10, i32::MIN, i32::MAX] {
            let output = render(&input, w, h, options(rect(x, y, 640, 480), 128)).unwrap();
            assert_eq!(output, untouched, "x {x} y {y}");
        }
    }
    for &y in &extremes {
        let output = render(&input, w, h, options(rect(5, y, 20, 20), 128)).unwrap();
        assert_eq!(output, untouched, "y {y}");
    }
    // Touching but not overlapping the frame.
    for bounds in [
        rect(-20, 0, 20, 20),
        rect(40, 0, 20, 20),
        rect(0, -20, 20, 20),
        rect(0, 30, 20, 20),
    ] {
        assert_eq!(
            render(&input, w, h, options(bounds, 128)).unwrap(),
            untouched
        );
    }
}

#[test]
fn bounds_larger_than_the_frame_and_tiny_bounds_are_safe() {
    let (w, h) = (9usize, 7usize);
    let input = input_frame(w, h);
    for bounds in [
        rect(-100, -100, 640, 480),
        rect(3, 2, 1, 1),
        rect(0, 0, 2, 1),
    ] {
        let a = render(&input, w, h, options(bounds, 128)).unwrap();
        let b = render(&input, w, h, options(bounds, 128)).unwrap();
        assert_eq!(a, b);
        assert!(a.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }
    // A 1 x 1 rectangle covers exactly one pixel.
    let output = render(&input, w, h, options(rect(3, 2, 1, 1), 128)).unwrap();
    let changed = output
        .as_chunks::<4>()
        .0
        .iter()
        .zip(input.as_chunks::<4>().0.iter())
        .filter(|(o, i)| o[..3] != i[..3])
        .count();
    assert_eq!(changed, 1);
}

#[test]
fn tiny_and_odd_frames_render() {
    for (w, h) in [(1usize, 1usize), (1, 5), (7, 1), (3, 5), (5, 3)] {
        let input = input_frame(w, h);
        let output = render(&input, w, h, options(rect(0, 0, w as u32, h as u32), 128)).unwrap();
        assert_eq!(output.len(), w * h * 4);
        assert!(output.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }
}

#[test]
fn maximum_frame_and_bounds_render() {
    let (w, h) = (640usize, 480usize);
    let input = input_frame(w, h);
    let mut output = vec![0u8; w * h * 4];
    render_skull_rgba(
        &input,
        &mut output,
        w,
        h,
        options(rect(0, 0, 640, 480), 128),
    )
    .unwrap();
    assert!(output.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    assert_ne!(output[..3], [0, 0, 0]);
    // Cranium center is covered by bone, so it differs from the input.
    let center = (100 * w + 320) * 4;
    assert_ne!(output[center..center + 3], input[center..center + 3]);
}

// --- Buffers and errors -----------------------------------------------------

#[test]
fn longer_output_keeps_its_suffix() {
    let (w, h) = (160usize, 120usize);
    let input = input_frame(w, h);
    let mut output = vec![0x5Au8; input.len() + 37];
    render_skull_rgba(
        &input,
        &mut output,
        w,
        h,
        options(rect(56, 28, 48, 64), 128),
    )
    .unwrap();
    assert!(output[input.len()..].iter().all(|&b| b == 0x5A));
    let m = mask160();
    assert_eq!(
        output[..input.len()],
        expected_rgba(&input, w, h, |x, y| m[y * w + x], 128)[..]
    );
}

fn assert_error(
    input_len: usize,
    output_len: usize,
    width: usize,
    height: usize,
    options: SkullOptions,
    error: SkullError,
) {
    let input = vec![9u8; input_len];
    let mut output = vec![0xC3u8; output_len];
    assert_eq!(
        render_skull_rgba(&input, &mut output, width, height, options),
        Err(error)
    );
    assert!(
        output.iter().all(|&b| b == 0xC3),
        "output changed on {error}"
    );
}

#[test]
fn invalid_arguments_return_errors_and_leave_output_intact() {
    let good = options(rect(0, 0, 2, 2), 128);
    assert_error(0, 16, 0, 2, good, SkullError::EmptyDimension);
    assert_error(0, 16, 2, 0, good, SkullError::EmptyDimension);
    assert_error(0, 0, 0, usize::MAX, good, SkullError::EmptyDimension);
    assert_error(0, 0, usize::MAX, 2, good, SkullError::DimensionsOverflow);
    assert_error(0, 0, 2, usize::MAX, good, SkullError::DimensionsOverflow);
    assert_error(
        0,
        0,
        usize::MAX / 4 + 1,
        1,
        good,
        SkullError::DimensionsOverflow,
    );
    assert_error(0, 0, 1usize << 20, 1, good, SkullError::DimensionsTooLarge);
    assert_error(0, 0, 641, 1, good, SkullError::DimensionsTooLarge);
    assert_error(0, 0, 1, 481, good, SkullError::DimensionsTooLarge);
    assert_error(15, 64, 2, 2, good, SkullError::InputLength);
    assert_error(17, 64, 2, 2, good, SkullError::InputLength);
    assert_error(16, 15, 2, 2, good, SkullError::OutputTooShort);
    assert_error(16, 0, 2, 2, good, SkullError::OutputTooShort);
    for bad in [
        rect(0, 0, 0, 2),
        rect(0, 0, 2, 0),
        rect(0, 0, 641, 2),
        rect(0, 0, 2, 481),
    ] {
        assert_error(16, 16, 2, 2, options(bad, 128), SkullError::InvalidBounds);
    }
    assert_error(
        16,
        16,
        2,
        2,
        options(rect(0, 0, u32::MAX, u32::MAX), 128),
        SkullError::InvalidBounds,
    );
    assert_error(
        16,
        16,
        2,
        2,
        options(rect(0, 0, 2, 2), 129),
        SkullError::OpacityOutOfRange,
    );
    assert_error(
        16,
        16,
        2,
        2,
        options(rect(0, 0, 2, 2), 255),
        SkullError::OpacityOutOfRange,
    );
}

#[test]
fn validation_order_is_documented_order() {
    let bad_all = options(rect(0, 0, 0, 0), 255);
    // Input length is checked before output capacity, bounds before opacity.
    assert_error(15, 4, 2, 2, bad_all, SkullError::InputLength);
    assert_error(16, 4, 2, 2, bad_all, SkullError::OutputTooShort);
    assert_error(16, 16, 2, 2, bad_all, SkullError::InvalidBounds);
    // Dimensions are checked before the buffers.
    assert_error(1, 1, 641, 1, bad_all, SkullError::DimensionsTooLarge);
}

#[test]
fn errors_implement_display_and_error() {
    let all = [
        SkullError::EmptyDimension,
        SkullError::DimensionsOverflow,
        SkullError::DimensionsTooLarge,
        SkullError::InputLength,
        SkullError::OutputTooShort,
        SkullError::InvalidBounds,
        SkullError::OpacityOutOfRange,
    ];
    for error in all {
        assert!(!error.to_string().is_empty());
        let _: &dyn std::error::Error = &error;
    }
    let copy = all[0];
    assert_eq!(copy, all[0]);
    let o = options(rect(1, -2, 3, 4), 5);
    assert_eq!(o, o.clone());
    assert!(format!("{o:?}").contains("opacity"));
}

// --- Bounded generated cases --------------------------------------------------

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

#[test]
fn generated_cases_are_deterministic_and_do_not_panic() {
    let mut rng = Lcg(0x5EED_0001);
    for case in 0..400 {
        let w = 1 + (rng.next() % 12) as usize;
        let h = 1 + (rng.next() % 12) as usize;
        let input = input_frame(w, h);
        let x = match rng.next() % 4 {
            0 => i32::MIN + (rng.next() % 3) as i32,
            1 => i32::MAX - (rng.next() % 3) as i32,
            _ => (rng.next() % 40) as i32 - 20,
        };
        let y = (rng.next() % 40) as i32 - 20;
        let bounds = rect(x, y, 1 + rng.next() % 30, 1 + rng.next() % 30);
        let opacity = (rng.next() % 129) as u8;
        let extra = (rng.next() % 5) as usize;
        let mut a = vec![0x77u8; input.len() + extra];
        let mut b = vec![0x11u8; input.len() + extra];
        render_skull_rgba(&input, &mut a, w, h, options(bounds, opacity)).unwrap();
        render_skull_rgba(&input, &mut b, w, h, options(bounds, opacity)).unwrap();
        assert_eq!(a[..input.len()], b[..input.len()], "case {case}");
        assert!(a[input.len()..].iter().all(|&v| v == 0x77));
        assert!(a[..input.len()]
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] == 255));
        if opacity == 0 {
            for (o, i) in a
                .as_chunks::<4>()
                .0
                .iter()
                .zip(input.as_chunks::<4>().0.iter())
            {
                assert_eq!(o[..3], i[..3]);
            }
        }
    }
}
