//! Native adapter for the shared progression, director, history and skull renderer.
use crate::artwork::{green, mix};
use crate::presence::{Face, FRESH_MS};
use spook_core::{
    motion_gray8, render_skull_rgba, DirectorConfig, DirectorInput, HauntDirector,
    HauntProgression, OverlayRect, SkullOptions, TemporalHistory, TemporalOptions, SKULL,
};

pub struct Haunting {
    width: usize,
    height: usize,
    progression: HauntProgression,
    director: HauntDirector,
    history: TemporalHistory,
    previous: Vec<u8>,
    gray: Vec<u8>,
    work: Vec<u8>,
    mirrored: Vec<u8>,
    scratch: Vec<u8>,
    sampled: Option<u64>,
    activity: u16,
    skull_start: Option<u64>,
    skull_positive: Option<u64>,
}

impl Haunting {
    pub fn new(width: usize, height: usize, seed: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let n = width * height;
        Ok(Self {
            width,
            height,
            progression: HauntProgression::with_seed(seed)?,
            director: HauntDirector::new(seed, DirectorConfig::default())?,
            history: TemporalHistory::new(width, height)?,
            previous: vec![0; n],
            gray: vec![0; n],
            work: vec![0; n * 4],
            mirrored: vec![0; n * 4],
            scratch: vec![0; n * 4],
            sampled: None,
            activity: 0,
            skull_start: None,
            skull_positive: None,
        })
    }
    pub fn reset(&mut self) {
        self.progression.reset();
        self.director.reset();
        self.history.reset();
        self.sampled = None;
        self.activity = 0;
        self.skull_start = None;
        self.skull_positive = None;
        self.work.fill(0);
        self.mirrored.fill(0);
        self.scratch.fill(0);
        self.gray.fill(0);
        self.previous.fill(0);
    }
    pub fn process(
        &mut self,
        rgba: &[u8],
        now: u64,
        face: Option<(u64, Face)>,
        reduced: bool,
        out: &mut [u32],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if rgba.len() != self.width * self.height * 4 || out.len() != self.width * self.height {
            return Err("invalid native haunting frame".into());
        }
        for (dst, p) in self.gray.iter_mut().zip(rgba.as_chunks::<4>().0) {
            *dst = ((u32::from(p[0]) * 77 + u32::from(p[1]) * 150 + u32::from(p[2]) * 29 + 128)
                >> 8) as u8;
        }
        if self
            .sampled
            .is_none_or(|last| now.saturating_sub(last) >= 100)
        {
            self.activity = if self.sampled.is_some() {
                motion_gray8(
                    &self.gray,
                    &self.previous,
                    self.width,
                    self.height,
                    self.width,
                    12,
                )?
                .activity
            } else {
                0
            };
            self.previous.copy_from_slice(&self.gray);
            self.sampled = Some(now);
        }
        let face = face.filter(|(at, f)| *at <= now && now - *at <= FRESH_MS && f.valid());
        // Face presence keeps a still visitor's progression active; director motion stays measured.
        let mut level = self.progression.update(
            now,
            if face.is_some() {
                self.activity.max(1000)
            } else {
                self.activity
            },
            reduced,
        )?;
        level.activity = self.activity;
        let snapshot = self.director.update(
            now,
            DirectorInput {
                level,
                head_available: face.is_some(),
                reduced_motion: reduced,
                quality_budget: 2,
            },
        )?;
        self.history.process(
            rgba,
            &mut self.work,
            now,
            TemporalOptions {
                delay_ms: snapshot.delay_ms,
                trail_strength: snapshot.trail_strength,
            },
        )?;
        let intensity = u32::from(level.strength) * 140 / 6000;
        for y in 0..self.height {
            for x in 0..self.width {
                let src = (y * self.width + self.width - 1 - x) * 4;
                let dst = (y * self.width + x) * 4;
                let p = &self.work[src..src + 4];
                let rgb = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
                let gray =
                    ((u32::from(p[0]) * 77 + u32::from(p[1]) * 150 + u32::from(p[2]) * 29 + 128)
                        >> 8) as u8;
                let color = mix(rgb, green(gray), intensity);
                self.mirrored[dst..dst + 4].copy_from_slice(&[
                    (color >> 16) as u8,
                    (color >> 8) as u8,
                    color as u8,
                    255,
                ]);
            }
        }
        let selected = snapshot.layers & SKULL != 0 && snapshot.strength > 0;
        let opacity = self.apparition(now, snapshot.strength, selected, face.is_some() && !reduced);
        if opacity > 0 {
            if let Some((_, f)) = face {
                let sx = self.width as u32 / 160;
                let sy = self.height as u32 / 120;
                let bounds = OverlayRect {
                    x: (self.width as u32 - (f.x + f.width) * sx) as i32,
                    y: (f.y * sy) as i32,
                    width: f.width * sx,
                    height: f.height * sy,
                };
                self.scratch.copy_from_slice(&self.mirrored);
                render_skull_rgba(
                    &self.scratch,
                    &mut self.mirrored,
                    self.width,
                    self.height,
                    SkullOptions { bounds, opacity },
                )?;
            }
        }
        for (dst, p) in out.iter_mut().zip(self.mirrored.as_chunks::<4>().0) {
            *dst = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
        }
        Ok(())
    }

    fn apparition(&mut self, now: u64, strength: u16, selected: bool, usable: bool) -> u8 {
        if !selected {
            return 0;
        }
        // Shared director policy separates SKULL events by at least 8000 ms.
        if self
            .skull_positive
            .is_none_or(|last| now.saturating_sub(last) >= 8000)
        {
            self.skull_start = None;
        }
        self.skull_positive = Some(now);
        if !usable {
            return 0;
        }
        let start = *self.skull_start.get_or_insert(now);
        let elapsed = now.saturating_sub(start);
        let ramp = if elapsed < 1200 {
            elapsed
        } else if elapsed < 2000 {
            1200
        } else {
            3200_u64.saturating_sub(elapsed)
        };
        ((96 * ramp * u64::from(strength.min(6000)) + 3_600_000) / 7_200_000) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_skull_cannot_retrigger_and_honors_selection_and_freshness() {
        let mut e = Haunting::new(160, 120, 1).unwrap();
        for (now, expected) in [
            (0, 0),
            (600, 48),
            (1200, 96),
            (1999, 96),
            (2000, 96),
            (2600, 48),
            (3200, 0),
            (5000, 0),
            (7999, 0),
        ] {
            assert_eq!(e.apparition(now, 6000, true, true), expected, "{now}");
        }
        assert_eq!(e.apparition(8000, 6000, false, true), 0);
        assert_eq!(e.apparition(8001, 6000, true, false), 0);
        assert_eq!(e.apparition(8002, 6000, true, true), 0);
        assert_eq!(e.apparition(16002, 6000, true, true), 0);
        assert_eq!(e.apparition(17202, 6000, true, true), 96);
        e.reset();
        assert_eq!(e.apparition(0, 6000, true, true), 0);
        assert_eq!(e.apparition(1200, 3000, true, true), 48);
    }

    #[test]
    fn stationary_face_drives_haunting_and_seed_replays_without_new_storage() {
        let mut a = Haunting::new(160, 120, 9).unwrap();
        let mut b = Haunting::new(160, 120, 9).unwrap();
        let frame = [40, 50, 60, 255].repeat(160 * 120);
        let mut left = vec![0; 160 * 120];
        let mut right = left.clone();
        let pointer = a.work.as_ptr();
        let mut changed = false;
        for now in (0..35000).step_by(200) {
            let face = Some((
                now,
                Face {
                    x: 40,
                    y: 20,
                    width: 40,
                    height: 50,
                    confidence: 900,
                },
            ));
            a.process(&frame, now, face, false, &mut left).unwrap();
            b.process(&frame, now, face, false, &mut right).unwrap();
            assert_eq!(left, right);
            changed |= left[0] != 0x28323c;
        }
        assert!(changed, "still faces must start the haunting progression");
        assert_eq!(pointer, a.work.as_ptr());
        a.reset();
        assert!(a.work.iter().all(|b| *b == 0));
    }
    #[test]
    fn opening_reduced_motion_mirroring_and_reset_use_real_pixels() {
        for (w, h) in [(160, 120), (320, 240)] {
            let mut engine = Haunting::new(w, h, 3).unwrap();
            let mut input = vec![0; w * h * 4];
            for p in input.as_chunks_mut::<4>().0 {
                p.copy_from_slice(&[10, 20, 30, 255]);
            }
            input[..4].copy_from_slice(&[255, 0, 0, 255]);
            let mut out = vec![0; w * h];
            engine.process(&input, 0, None, false, &mut out).unwrap();
            assert_eq!(out[w - 1], 0xff0000);
            assert_eq!(out[0], 0x0a141e);
            engine
                .process(&input, 10_000, None, true, &mut out)
                .unwrap();
            assert_eq!(out[w - 1], 0xff0000);
            engine.reset();
            engine.process(&input, 0, None, false, &mut out).unwrap();
            assert_eq!(out[w - 1], 0xff0000);
        }
    }
}
