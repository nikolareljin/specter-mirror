use crate::artwork::{mix, Artwork};
use crate::haunting::Haunting;
use crate::presence::{Face, Presence};

pub struct IdleDisplay {
    artwork: Artwork,
    haunting: Haunting,
    presence: Presence,
    input: Vec<u8>,
    live_pixels: Vec<u32>,
    pixels: Vec<u32>,
    changed: u64,
    from_weight: u32,
    weight: u32,
    reduced: bool,
    has_frame: bool,
    available: bool,
}

impl IdleDisplay {
    pub fn new(
        width: usize,
        height: usize,
        seed: u32,
        reduced: bool,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            artwork: Artwork::new(width, height)?,
            haunting: Haunting::new(width, height, seed)?,
            presence: Presence::default(),
            input: vec![0; width * height * 4],
            live_pixels: vec![0; width * height],
            pixels: vec![0; width * height],
            changed: 0,
            from_weight: 0,
            weight: 0,
            reduced,
            has_frame: false,
            available: false,
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
        if let Some(frame) = frame {
            if frame.len() != self.input.len() {
                return Err("invalid idle camera frame".into());
            }
        }
        let was_live = self.live();
        if !available {
            if self.available || self.has_frame {
                self.presence.reset();
                self.haunting.reset();
                self.input.fill(0);
                self.has_frame = false;
                self.weight = 0;
                self.from_weight = 0;
                self.live_pixels.fill(0);
            }
        } else {
            if let Some(frame) = frame {
                self.input.copy_from_slice(frame);
                self.has_frame = true;
            }
            self.presence
                .tick(now, observation.filter(|_| self.has_frame))?;
        }
        self.available = available;
        if self.live() != was_live {
            self.changed = now;
            self.from_weight = self.weight;
            self.haunting.reset();
        }
        if self.live() && self.has_frame {
            self.haunting
                .process(&self.input, now, face, self.reduced, &mut self.live_pixels)?;
        }
        let target = if self.live() { 255 } else { 0 };
        self.weight = if self.reduced {
            target
        } else {
            let progress = (now.saturating_sub(self.changed).min(1000) * 255 / 1000) as u32;
            (self.from_weight * (255 - progress) + target * progress + 127) / 255
        };
        self.artwork.render(now, self.reduced, &mut self.pixels);
        for (dst, live) in self.pixels.iter_mut().zip(&self.live_pixels) {
            *dst = mix(*dst, *live, self.weight);
        }
        if !self.live() && self.weight == 0 {
            self.live_pixels.fill(0);
        }
        Ok(())
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
    fn real_camera_only_activation_returns_to_art_and_forgets_previous_visit() {
        for (w, h) in [(160, 120), (320, 240)] {
            let mut display = IdleDisplay::new(w, h, 7, false).unwrap();
            let rgba = [250, 10, 20, 255].repeat(w * h);
            display.update(0, None, None, false, None).unwrap();
            let art = display.pixels.clone();
            for at in (100..=2100).step_by(200) {
                display
                    .update(
                        at,
                        Some(&rgba),
                        Some((at, Some(face()))),
                        true,
                        Some((at, face())),
                    )
                    .unwrap();
                assert!(!display.live());
            }
            display
                .update(
                    2101,
                    Some(&rgba),
                    Some((2101, Some(face()))),
                    true,
                    Some((2101, face())),
                )
                .unwrap();
            assert!(display.live());
            display.update(3101, None, None, true, None).unwrap();
            assert_eq!(display.weight, 255);
            assert_eq!(
                display.pixels[0], 0xfa0a14,
                "neutral live opening must use camera pixels"
            );
            display.update(5101, None, None, true, None).unwrap();
            assert!(!display.live());
            display.update(6101, None, None, true, None).unwrap();
            assert_eq!(display.pixels, art);
            display.update(6200, None, None, false, None).unwrap();
            assert!(display.input.iter().all(|v| *v == 0));
            assert_eq!(display.pixels, art);
        }
    }
    #[test]
    fn art_and_stale_results_cannot_start_a_session_and_buffers_are_reused() {
        let mut d = IdleDisplay::new(160, 120, 1, true).unwrap();
        let ptr = d.pixels.as_ptr();
        d.update(0, None, None, false, None).unwrap();
        let expected = d.pixels.clone();
        for now in (100..10000).step_by(100) {
            d.update(now, None, None, false, None).unwrap();
            assert!(!d.live());
            assert_eq!(d.pixels, expected);
        }
        assert_eq!(ptr, d.pixels.as_ptr());
        let rgba = [10, 20, 30, 255].repeat(160 * 120);
        d.update(10000, Some(&rgba), Some((0, Some(face()))), true, None)
            .unwrap();
        assert!(!d.live());
        assert_eq!(d.pixels, expected);
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
}
