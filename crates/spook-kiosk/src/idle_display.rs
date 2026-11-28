use crate::artwork::{fade_weight, mix, Artwork};
use crate::haunting::Haunting;
use crate::presence::{Face, Presence};

const REVEAL_MS: u64 = 1000;
const RETURN_MS: u64 = 1000;
const RESTORE_MS: u64 = 6000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Reveal(u64),
    Live,
    Return(u64),
    Restore(u64),
}

pub struct IdleDisplay {
    artwork: Artwork,
    haunting: Haunting,
    presence: Presence,
    input: Vec<u8>,
    live_pixels: Vec<u32>,
    pixels: Vec<u32>,
    transition_from: Vec<u32>,
    phase: Phase,
    reduced: bool,
    has_frame: bool,
    available: bool,
    last_now: Option<u64>,
}

impl IdleDisplay {
    pub fn new(
        width: usize,
        height: usize,
        seed: u32,
        reduced: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let artwork = Artwork::new(width, height, seed)?;
        let pixels = artwork.natural().to_vec();
        Ok(Self {
            artwork,
            haunting: Haunting::new(width, height, seed)?,
            presence: Presence::default(),
            input: vec![0; width * height * 4],
            live_pixels: vec![0; width * height],
            transition_from: vec![0; width * height],
            pixels,
            phase: Phase::Idle,
            reduced,
            has_frame: false,
            available: false,
            last_now: None,
        })
    }
    pub fn live(&self) -> bool {
        self.presence.live()
    }
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    pub fn update(
        &mut self,
        now: u64,
        frame: Option<&[u8]>,
        observation: Option<(u64, Option<Face>)>,
        available: bool,
        face: Option<(u64, Face)>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.last_now.is_some_and(|last| now < last) {
            return Err("display time regressed".into());
        }
        if frame.is_some_and(|p| p.len() != self.input.len()) {
            return Err("invalid idle camera frame".into());
        }
        let was_live = self.live();
        if available {
            self.presence.tick(
                now,
                observation.filter(|_| self.has_frame || frame.is_some()),
            )?;
            if let Some(frame) = frame {
                self.input.copy_from_slice(frame);
                self.has_frame = true;
            }
        } else if self.available || self.has_frame {
            self.presence.reset();
            self.haunting.reset();
            self.input.fill(0);
            self.live_pixels.fill(0);
            self.transition_from.fill(0);
            self.has_frame = false;
            self.phase = Phase::Idle;
            self.artwork.settle();
        }
        let delta = self.last_now.map_or(0, |last| now - last);
        if self.live() != was_live && available {
            self.transition_from.copy_from_slice(&self.pixels);
            self.haunting.reset();
            if self.live() {
                self.artwork.settle();
                self.phase = if self.reduced {
                    Phase::Live
                } else {
                    Phase::Reveal(now)
                };
            } else if self.reduced {
                self.phase = Phase::Idle;
                self.artwork.restart_hold();
            } else {
                self.phase = Phase::Return(now);
            }
        } else if self.phase == Phase::Idle && (!self.available || available) {
            self.artwork.advance(delta, self.reduced);
        }
        self.available = available;
        if self.live() && self.has_frame {
            self.haunting
                .process(&self.input, now, face, self.reduced, &mut self.live_pixels)?;
        }
        match self.phase {
            Phase::Idle => self.artwork.render(&mut self.pixels),
            Phase::Reveal(start) => {
                let elapsed = now - start;
                blend(
                    &self.transition_from,
                    &self.live_pixels,
                    fade_weight(elapsed, REVEAL_MS as u16),
                    &mut self.pixels,
                );
                if elapsed >= REVEAL_MS {
                    self.phase = Phase::Live;
                }
            }
            Phase::Live => self.pixels.copy_from_slice(&self.live_pixels),
            Phase::Return(start) => {
                let elapsed = now - start;
                if elapsed < RETURN_MS {
                    blend(
                        &self.transition_from,
                        self.artwork.tinted(),
                        fade_weight(elapsed, RETURN_MS as u16),
                        &mut self.pixels,
                    );
                } else {
                    self.phase = Phase::Restore(start.saturating_add(RETURN_MS));
                    self.restore(elapsed - RETURN_MS);
                }
            }
            Phase::Restore(start) => self.restore(now - start),
        }
        self.last_now = Some(now);
        Ok(())
    }

    fn restore(&mut self, elapsed: u64) {
        blend(
            self.artwork.tinted(),
            self.artwork.natural(),
            fade_weight(elapsed, RESTORE_MS as u16),
            &mut self.pixels,
        );
        if elapsed >= RESTORE_MS {
            self.phase = Phase::Idle;
            self.artwork.restart_hold();
            self.live_pixels.fill(0);
            self.transition_from.fill(0);
        }
    }
}

fn blend(from: &[u32], to: &[u32], weight: u32, out: &mut [u32]) {
    for ((dst, a), b) in out.iter_mut().zip(from).zip(to) {
        *dst = mix(*a, *b, weight);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn face() -> Face {
        Face {
            x: 10,
            y: 10,
            width: 40,
            height: 50,
            confidence: 900,
        }
    }

    #[test]
    fn a_long_live_visit_keeps_the_same_photo_reserved_for_recovery() {
        let mut d = IdleDisplay::new(160, 120, 5, false).unwrap();
        let frame = [50, 100, 150, 255].repeat(160 * 120);
        let held = d.artwork.natural().as_ptr();
        enter(&mut d, &frame);
        d.update(3001, None, None, true, None).unwrap();
        d.update(
            200000,
            Some(&frame),
            Some((200000, Some(face()))),
            true,
            Some((200000, face())),
        )
        .unwrap();
        assert!(d.live());
        assert_eq!(
            d.artwork.natural().as_ptr(),
            held,
            "hidden slideshow advanced during the visit"
        );
    }
    fn enter(d: &mut IdleDisplay, frame: &[u8]) {
        for at in [0, 400, 800, 1200, 1600, 2000] {
            d.update(
                at,
                Some(frame),
                Some((at, Some(face()))),
                true,
                Some((at, face())),
            )
            .unwrap();
            assert!(!d.live());
        }
        d.update(
            2001,
            Some(frame),
            Some((2001, Some(face()))),
            true,
            Some((2001, face())),
        )
        .unwrap();
        assert!(d.live());
    }
    #[test]
    fn natural_photo_full_color_video_green_photo_then_slow_natural_recovery() {
        for (w, h) in [(160, 120), (320, 240)] {
            let mut d = IdleDisplay::new(w, h, 7, false).unwrap();
            let frame = [250, 10, 20, 255].repeat(w * h);
            let natural = d.pixels.clone();
            let tinted = d.artwork.tinted().to_vec();
            let current = d.artwork.natural().as_ptr();
            enter(&mut d, &frame);
            assert_eq!(
                d.pixels, natural,
                "reveal must start at the displayed natural photo"
            );
            d.update(3001, None, None, true, None).unwrap();
            assert_eq!(
                d.pixels[0], 0xfa0a14,
                "the webcam opening must be full color"
            );
            let live = d.pixels.clone();
            d.update(5001, None, None, true, None).unwrap();
            assert!(!d.live());
            assert_eq!(d.pixels, live);
            d.update(6001, None, None, true, None).unwrap();
            assert_eq!(d.pixels, tinted, "return must reach the fully green photo");
            d.update(9001, None, None, true, None).unwrap();
            for i in 0..w * h {
                assert_eq!(d.pixels[i], mix(tinted[i], natural[i], 128));
            }
            assert_eq!(d.artwork.natural().as_ptr(), current);
            d.update(12001, None, None, true, None).unwrap();
            assert_eq!(d.phase, Phase::Idle);
            assert_eq!(d.pixels, natural);
            d.update(132001, None, None, true, None).unwrap();
            assert_eq!(
                d.pixels, natural,
                "the recovered image gets a new two-minute hold"
            );
            d.update(137001, None, None, true, None).unwrap();
            assert_ne!(d.artwork.natural().as_ptr(), current);
        }
    }
    #[test]
    fn entry_during_recovery_continues_from_the_visible_frame_and_reduced_motion_is_immediate() {
        for reduced in [false, true] {
            let mut d = IdleDisplay::new(160, 120, 4, reduced).unwrap();
            let frame = [50, 100, 150, 255].repeat(160 * 120);
            let natural = d.pixels.clone();
            enter(&mut d, &frame);
            if reduced {
                assert_eq!(d.pixels[0], 0x326496);
            }
            d.update(3001, None, None, true, None).unwrap();
            d.update(5001, None, None, true, None).unwrap();
            if reduced {
                assert_eq!(d.pixels, natural);
                assert_eq!(d.phase, Phase::Idle);
            } else {
                d.update(6001, None, None, true, None).unwrap();
                assert_eq!(d.pixels, d.artwork.tinted());
            }
            for at in [6100, 6500, 6900, 7300, 7700, 8100] {
                d.update(
                    at,
                    Some(&frame),
                    Some((at, Some(face()))),
                    true,
                    Some((at, face())),
                )
                .unwrap();
                assert!(!d.live());
            }
            let before = d.pixels.clone();
            d.update(
                8101,
                Some(&frame),
                Some((8101, Some(face()))),
                true,
                Some((8101, face())),
            )
            .unwrap();
            assert!(d.live());
            if !reduced {
                assert_eq!(
                    d.pixels, before,
                    "re-entry may not jump to a different image"
                );
            }
        }
    }
    #[test]
    fn failure_stale_observations_invalid_time_and_buffers_are_safe() {
        let mut d = IdleDisplay::new(160, 120, 2, false).unwrap();
        let frame = [10, 20, 30, 255].repeat(160 * 120);
        let pointer = d.pixels.as_ptr();
        enter(&mut d, &frame);
        d.update(3001, None, None, true, None).unwrap();
        d.update(3002, None, None, false, None).unwrap();
        assert_eq!(d.phase, Phase::Idle);
        assert_eq!(d.pixels, d.artwork.natural());
        assert!(d.input.iter().all(|b| *b == 0));
        let shown = d.pixels.clone();
        assert!(d.update(3001, None, None, true, None).is_err());
        assert_eq!(d.pixels, shown);
        assert!(d.update(3003, Some(&frame[..4]), None, true, None).is_err());
        assert_eq!(d.pixels, shown);
        d.update(10000, Some(&frame), Some((0, Some(face()))), true, None)
            .unwrap();
        assert!(!d.live());
        assert_eq!(pointer, d.pixels.as_ptr());
        for at in (10200..20000).step_by(200) {
            d.update(at, None, None, true, None).unwrap();
            assert!(!d.live());
        }
    }

    #[test]
    fn observations_without_live_input_cannot_activate_the_display() {
        let mut d = IdleDisplay::new(160, 120, 1, false).unwrap();
        for at in (0..10000).step_by(200) {
            d.update(at, None, Some((at, Some(face()))), true, Some((at, face())))
                .unwrap();
            assert!(!d.live());
        }
    }

    #[test]
    fn reduced_motion_idle_stays_natural_without_shuffling_or_allocating() {
        let mut d = IdleDisplay::new(160, 120, 3, true).unwrap();
        let expected = d.pixels.clone();
        let pointer = d.pixels.as_ptr();
        for at in [0, 120000, 125000, 500000, u64::MAX] {
            d.update(at, None, None, false, None).unwrap();
            assert_eq!(d.pixels, expected);
            assert_eq!(d.pixels.as_ptr(), pointer);
        }
    }
}
