//! Native tests for the browser face detector. They check validation and
//! determinism; they do not prove accuracy on real faces.
#![cfg(feature = "face-detection")]
use spook_web::BrowserFaceDetector;

/// Smooth original gradient with a few textured stripes; not a face.
fn pattern(w: u32, h: u32) -> Vec<u8> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            ((x * 3 + y * 5 + (x * y) % 29) % 256) as u8
        })
        .collect()
}

#[test]
fn rejects_invalid_dimensions() {
    for (w, h) in [
        (0, 120),
        (160, 0),
        (0, 0),
        (161, 120),
        (160, 121),
        (320, 240),
    ] {
        assert!(BrowserFaceDetector::new(w, h).is_err(), "{w}x{h}");
    }
}

#[test]
fn accepts_boundary_dimensions_without_panicking() {
    for (w, h) in [(1, 1), (160, 1), (1, 120), (19, 19), (160, 120), (80, 60)] {
        let mut d = BrowserFaceDetector::new(w, h).expect("constructs");
        assert!(d
            .detect_gray8(&vec![128; (w * h) as usize])
            .unwrap()
            .is_empty());
    }
}

#[test]
fn rejects_wrong_lengths() {
    let mut d = BrowserFaceDetector::new(160, 120).unwrap();
    let full = 160 * 120;
    for len in [0, 1, full - 1, full + 1, full * 4] {
        let err = d.detect_gray8(&vec![0; len]).unwrap_err();
        assert!(err.contains("19200"), "{err}");
    }
    // The detector stays usable after a rejected buffer.
    assert!(d.detect_gray8(&vec![0; full]).unwrap().is_empty());
}

#[test]
fn blank_frames_have_no_face() {
    let mut d = BrowserFaceDetector::new(160, 120).unwrap();
    for v in [0u8, 128, 255] {
        assert!(d.detect_gray8(&vec![v; 160 * 120]).unwrap().is_empty());
    }
}

#[test]
fn output_is_deterministic_and_well_formed() {
    let frame = pattern(160, 120);
    let mut a = BrowserFaceDetector::new(160, 120).unwrap();
    let mut b = BrowserFaceDetector::new(160, 120).unwrap();
    let first = a.detect_gray8(&frame).unwrap();
    assert_eq!(first, a.detect_gray8(&frame).unwrap(), "reused detector");
    assert_eq!(first, b.detect_gray8(&frame).unwrap(), "fresh detector");
    if !first.is_empty() {
        assert_eq!(first.len(), 5);
        assert!(first[2] > 0 && first[3] > 0);
        assert!(first[0] + first[2] <= 160 && first[1] + first[3] <= 120);
        assert!((800..=1000).contains(&first[4]));
    }
}
