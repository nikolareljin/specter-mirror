use spook_web::MirrorEngine;

const W: u32 = 16;
const H: u32 = 12;

fn frame(time: u64) -> Vec<u8> {
    let flip = (time / 100) % 2 == 1;
    (0..W * H)
        .flat_map(|i| {
            let v = if flip {
                255 - (i * 7 % 256) as u8
            } else {
                (i * 7 % 256) as u8
            };
            [v, v / 2, 255 - v, 255]
        })
        .collect()
}

fn engine(seed: Option<u32>) -> MirrorEngine {
    let mut engine = MirrorEngine::new(W, H).unwrap();
    engine.configure_automatic(true, false).unwrap();
    if let Some(seed) = seed {
        engine.configure_seed(seed).unwrap();
    }
    engine
}

fn strengths(engine: &mut MirrorEngine, until: u64) -> Vec<u16> {
    (0..=until)
        .step_by(100)
        .map(|t| {
            engine.process_automatic_rgba(&frame(t), t as f64).unwrap();
            engine.haunt_strength()
        })
        .collect()
}

#[test]
fn seed_requires_automatic_and_rejects_nothing_else() {
    let mut plain = MirrorEngine::new(W, H).unwrap();
    assert!(plain.configure_seed(1).is_err());
    assert!(engine(None).configure_seed(u32::MAX).is_ok());
}

#[test]
fn same_seed_replays_and_different_seeds_differ() {
    let a = strengths(&mut engine(Some(9)), 40_000);
    let b = strengths(&mut engine(Some(9)), 40_000);
    assert_eq!(a, b);
    let others: Vec<_> = (10..16)
        .map(|s| strengths(&mut engine(Some(s)), 40_000))
        .collect();
    assert!(others.iter().any(|o| *o != a));
    let first_rise = |v: &[u16]| v.iter().position(|&s| s > 0);
    let starts: std::collections::HashSet<_> = (0..30)
        .map(|s| first_rise(&strengths(&mut engine(Some(s)), 40_000)))
        .collect();
    assert!(starts.len() > 5, "opening lengths vary: {starts:?}");
    for start in starts.into_iter().flatten() {
        // Never before 4 s of clear mirror.
        assert!(start * 100 >= 4000, "{start}");
    }
}

#[test]
fn unseeded_engine_keeps_the_fixed_ramp() {
    let fixed = strengths(&mut engine(None), 12_000);
    assert_eq!(fixed[50], 0);
    assert_eq!(fixed[110], 6000);
}

#[test]
fn reseeding_and_reset_restart_the_session_with_the_same_timing() {
    let mut e = engine(Some(4));
    let first = strengths(&mut e, 30_000);
    e.reset_temporal();
    assert_eq!(strengths(&mut e, 30_000), first);
    e.configure_seed(4).unwrap();
    assert_eq!(strengths(&mut e, 30_000), first);
}

#[test]
fn seeded_strength_changes_gradually_per_frame() {
    for seed in 0..10 {
        let s = strengths(&mut engine(Some(seed)), 60_000);
        for pair in s.windows(2) {
            // 100 ms frames, shortest stage 4 s, smoothstep slope at most 1.5.
            assert!(pair[0].abs_diff(pair[1]) <= 240, "seed {seed}: {pair:?}");
        }
    }
}
