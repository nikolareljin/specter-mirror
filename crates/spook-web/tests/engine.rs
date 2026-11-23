use spook_web::{MirrorEngine, MODE_MIRROR, MODE_PORTRAIT, MODE_SPECTRAL};

/// Distinct pixels so orientation errors cannot hide.
fn distinct(w: usize, h: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..w * h {
        v.extend_from_slice(&[(i * 7 + 1) as u8, (i * 13 + 2) as u8, (i * 29 + 3) as u8, 9]);
    }
    v
}

/// Hard vertical step: left half black, right half white.
fn step(w: usize, h: usize) -> Vec<u8> {
    let mut v = Vec::new();
    for _ in 0..h {
        for x in 0..w {
            let c = if x < w / 2 { 0 } else { 255 };
            v.extend_from_slice(&[c, c, c, 255]);
        }
    }
    v
}

fn px(out: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
    let i = (y * w + x) * 4;
    [out[i], out[i + 1], out[i + 2], out[i + 3]]
}

#[test]
fn mirror_flips_horizontally_with_distinct_pixels_and_opaque_alpha() {
    let (w, h) = (5, 3);
    let input = distinct(w, h);
    let mut e = MirrorEngine::new(w as u32, h as u32).unwrap();
    let out = e.process_rgba(&input, MODE_MIRROR, 0).unwrap();
    assert_eq!(out.len(), input.len());
    for y in 0..h {
        for x in 0..w {
            let s = (y * w + (w - 1 - x)) * 4;
            assert_eq!(
                px(&out, w, x, y),
                [input[s], input[s + 1], input[s + 2], 255]
            );
        }
    }
    assert_ne!(px(&out, w, 0, 0), px(&out, w, 1, 0));
}

#[test]
fn mirror_ignores_intensity() {
    let input = distinct(4, 2);
    let mut e = MirrorEngine::new(4, 2).unwrap();
    let a = e.process_rgba(&input, MODE_MIRROR, 0).unwrap();
    let b = e.process_rgba(&input, MODE_MIRROR, 255).unwrap();
    assert_eq!(a, b);
}

#[test]
fn spectral_full_intensity_maps_edges_to_cyan_and_rest_to_black() {
    let (w, h) = (6, 5);
    let mut e = MirrorEngine::new(w as u32, h as u32).unwrap();
    let out = e.process_rgba(&step(w, h), MODE_SPECTRAL, 255).unwrap();
    // Sobel keeps borders at zero; step between x=2 and x=3 saturates in the interior.
    // Source column 2 or 3 appears mirrored at output column 3 or 2.
    for y in 1..h - 1 {
        assert_eq!(px(&out, w, 2, y), [114, 231, 214, 255]);
        assert_eq!(px(&out, w, 3, y), [114, 231, 214, 255]);
        assert_eq!(px(&out, w, 1, y), [0, 0, 0, 255]);
        assert_eq!(px(&out, w, 4, y), [0, 0, 0, 255]);
    }
    for x in 0..w {
        assert_eq!(px(&out, w, x, 0), [0, 0, 0, 255]);
        assert_eq!(px(&out, w, x, h - 1), [0, 0, 0, 255]);
    }
}

#[test]
fn spectral_edge_is_mirrored_exactly_once() {
    // 7 wide, edge column set only at source x=1..2 so asymmetry shows.
    let (w, h) = (7, 3);
    let mut input = Vec::new();
    for _ in 0..h {
        for x in 0..w {
            let c = if x < 2 { 0 } else { 255 };
            input.extend_from_slice(&[c, c, c, 255]);
        }
    }
    let mut e = MirrorEngine::new(w as u32, h as u32).unwrap();
    let out = e.process_rgba(&input, MODE_SPECTRAL, 255).unwrap();
    // Source edge pixels are x=1 and x=2; mirrored they land at x=5 and x=4.
    for x in 0..w {
        let want = if x == 4 || x == 5 {
            [114, 231, 214, 255]
        } else {
            [0, 0, 0, 255]
        };
        assert_eq!(px(&out, w, x, 1), want, "x={x}");
    }
}

#[test]
fn spectral_preserves_unsaturated_edges() {
    // A 10-level vertical step produces Sobel magnitude 40, not a saturated edge.
    let mut input = Vec::new();
    for _ in 0..3 {
        for value in [0, 0, 10, 10, 10] {
            input.extend_from_slice(&[value, value, value, 255]);
        }
    }
    let mut engine = MirrorEngine::new(5, 3).unwrap();
    let output = engine.process_rgba(&input, MODE_SPECTRAL, 255).unwrap();
    assert_eq!(px(&output, 5, 2, 1), [18, 36, 34, 255]);
    assert_eq!(px(&output, 5, 3, 1), [18, 36, 34, 255]);
}

#[test]
fn portrait_full_intensity_uses_bone_and_midnight_with_bayer_threshold() {
    let (w, h) = (4, 4);
    let mut input = Vec::new();
    for _ in 0..w * h {
        input.extend_from_slice(&[128, 128, 128, 255]);
    }
    let mut e = MirrorEngine::new(w as u32, h as u32).unwrap();
    let out = e.process_rgba(&input, MODE_PORTRAIT, 255).unwrap();
    // Source pattern for 128 (lit when matrix <= 7), then mirrored left-right.
    let source = [[1, 0, 1, 0], [0, 1, 0, 1], [1, 0, 1, 0], [0, 1, 0, 1]];
    for (y, row) in source.iter().enumerate() {
        for x in 0..w {
            let lit = row[w - 1 - x] == 1;
            let want = if lit {
                [229, 223, 209, 255]
            } else {
                [8, 13, 20, 255]
            };
            assert_eq!(px(&out, w, x, y), want, "({x},{y})");
        }
    }
}

#[test]
fn zero_intensity_matches_mirror_for_effect_modes() {
    let (w, h) = (6, 4);
    let input = distinct(w, h);
    let mut e = MirrorEngine::new(w as u32, h as u32).unwrap();
    let mirror = e.process_rgba(&input, MODE_MIRROR, 0).unwrap();
    assert_eq!(e.process_rgba(&input, MODE_SPECTRAL, 0).unwrap(), mirror);
    assert_eq!(e.process_rgba(&input, MODE_PORTRAIT, 0).unwrap(), mirror);
}

#[test]
fn midpoint_blend_uses_rounded_integer_formula() {
    // One gray pixel 100 in portrait mode: dither threshold at (0,0) is 8, so lit (bone).
    let input = [100, 100, 100, 255];
    let mut e = MirrorEngine::new(1, 1).unwrap();
    let out = e.process_rgba(&input, MODE_PORTRAIT, 128).unwrap();
    let blend = |o: u32, p: u32| ((o * 127 + p * 128 + 127) / 255) as u8;
    assert_eq!(
        out,
        [blend(100, 229), blend(100, 223), blend(100, 209), 255]
    );
    assert_eq!(out, [165, 162, 155, 255]);
    // Dark pixel 4 is below threshold 8, so it blends toward midnight.
    let out = e.process_rgba(&[4, 4, 4, 255], MODE_PORTRAIT, 128).unwrap();
    assert_eq!(out, [blend(4, 8), blend(4, 13), blend(4, 20), 255]);
}

#[test]
fn invalid_construction_is_rejected() {
    for (w, h) in [(0, 1), (1, 0), (641, 1), (1, 481), (u32::MAX, u32::MAX)] {
        assert!(MirrorEngine::new(w, h).is_err(), "{w}x{h}");
    }
    assert!(MirrorEngine::new(640, 480).is_ok());
    assert!(MirrorEngine::new(1, 1).is_ok());
}

#[test]
fn invalid_mode_and_length_are_rejected_without_changing_later_results() {
    let (w, h) = (4, 3);
    let input = distinct(w, h);
    let mut e = MirrorEngine::new(w as u32, h as u32).unwrap();
    let before = e.process_rgba(&input, MODE_SPECTRAL, 200).unwrap();
    assert!(e.process_rgba(&input, 3, 255).is_err());
    assert!(e.process_rgba(&input, 255, 0).is_err());
    assert!(e.process_rgba(&input[..input.len() - 1], 0, 0).is_err());
    let mut longer = input.clone();
    longer.push(0);
    assert!(e.process_rgba(&longer, 0, 0).is_err());
    assert!(e.process_rgba(&[], 1, 0).is_err());
    assert_eq!(e.process_rgba(&input, MODE_SPECTRAL, 200).unwrap(), before);
}

#[test]
fn buffer_reuse_is_deterministic_across_modes_and_inputs() {
    let (w, h) = (9, 7);
    let a = distinct(w, h);
    let b = step(w, h);
    let mut e = MirrorEngine::new(w as u32, h as u32).unwrap();
    let fresh = |input: &[u8], mode, k| {
        MirrorEngine::new(w as u32, h as u32)
            .unwrap()
            .process_rgba(input, mode, k)
            .unwrap()
    };
    for round in 0..3 {
        for mode in [MODE_SPECTRAL, MODE_PORTRAIT, MODE_MIRROR] {
            for input in [&a, &b] {
                let got = e.process_rgba(input, mode, 180).unwrap();
                assert_eq!(got, fresh(input, mode, 180), "round {round} mode {mode}");
            }
        }
    }
}
