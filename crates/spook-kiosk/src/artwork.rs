use jpeg_decoder::{Decoder, PixelFormat};
use std::io::Cursor;

pub const HOLD_MS: u64 = 120_000;
const FADE_MS: u64 = 5000;
const IMAGES: [&[u8]; 3] = [
    include_bytes!("../../../assets/artwork/island.jpg"),
    include_bytes!("../../../assets/artwork/monsters.jpg"),
    include_bytes!("../../../assets/artwork/skull.jpg"),
];

pub fn green(gray: u8) -> u32 {
    let level = u32::from(gray);
    let channel = |dark, lit| (dark * (255 - level) + lit * level + 127) / 255;
    (channel(8, 139) << 16) | (channel(13, 207) << 8) | channel(20, 131)
}

pub fn mix(a: u32, b: u32, weight: u32) -> u32 {
    let c = |shift: u32| {
        ((((a >> shift) & 255) * (255 - weight) + ((b >> shift) & 255) * weight + 127) / 255)
            << shift
    };
    c(16) | c(8) | c(0)
}

/// Cubic easing with zero endpoint slope. The u16 duration bounds all products.
pub fn fade_weight(elapsed: u64, duration: u16) -> u32 {
    let d = u64::from(duration);
    if d == 0 {
        return 255;
    }
    let t = elapsed.min(d);
    let denominator = d * d * d;
    ((255 * t * t * (3 * d - 2 * t) + denominator / 2) / denominator) as u32
}

fn gray(pixel: u32) -> u8 {
    ((((pixel >> 16) & 255) * 77 + ((pixel >> 8) & 255) * 150 + (pixel & 255) * 29 + 128) >> 8)
        as u8
}

struct Shuffle {
    order: [usize; 3],
    following: [usize; 3],
    index: usize,
    random: u32,
}

impl Shuffle {
    fn new(seed: u32) -> Self {
        let mut shuffle = Self {
            order: [0, 1, 2],
            following: [0, 1, 2],
            index: 0,
            random: seed,
        };
        shuffle.order = shuffle.draw(None);
        shuffle.following = shuffle.draw(Some(shuffle.order[2]));
        shuffle
    }

    fn draw(&mut self, previous: Option<usize>) -> [usize; 3] {
        let mut order = [0, 1, 2];
        for i in (1..3).rev() {
            self.random = self.random.wrapping_mul(1664525).wrapping_add(1013904223);
            order.swap(i, (self.random >> 16) as usize % (i + 1));
        }
        if previous == Some(order[0]) {
            order.swap(0, 1);
        }
        order
    }

    fn current(&self) -> usize {
        self.order[self.index]
    }
    fn next(&self) -> usize {
        if self.index < 2 {
            self.order[self.index + 1]
        } else {
            self.following[0]
        }
    }
    fn advance(&mut self) {
        if self.index < 2 {
            self.index += 1;
        } else {
            self.order = self.following;
            self.following = self.draw(Some(self.order[2]));
            self.index = 0;
        }
    }
}

pub struct Artwork {
    natural: [Vec<u32>; 3],
    tinted: [Vec<u32>; 3],
    shuffle: Shuffle,
    elapsed: u64,
}

impl Artwork {
    pub fn new(width: usize, height: usize, seed: u32) -> Result<Self, String> {
        if ![(160, 120), (320, 240)].contains(&(width, height)) {
            return Err("unsupported artwork size".into());
        }
        let mut natural = [Vec::new(), Vec::new(), Vec::new()];
        for (dst, jpeg) in natural.iter_mut().zip(IMAGES) {
            *dst = decode(jpeg, width, height)?;
        }
        let tinted =
            std::array::from_fn(|i| natural[i].iter().map(|pixel| green(gray(*pixel))).collect());
        Ok(Self {
            natural,
            tinted,
            shuffle: Shuffle::new(seed),
            elapsed: 0,
        })
    }

    /// Only idle display time advances the playlist. Missed slides are not replayed.
    pub fn advance(&mut self, delta: u64, reduced: bool) {
        if reduced {
            return;
        }
        self.elapsed = self.elapsed.saturating_add(delta);
        if self.elapsed >= HOLD_MS + FADE_MS {
            self.shuffle.advance();
            self.elapsed = 0;
        }
    }

    fn crossfade(&self) -> u32 {
        fade_weight(self.elapsed.saturating_sub(HOLD_MS), FADE_MS as u16)
    }

    /// Freeze the dominant image of an interrupted crossfade for this visit.
    pub fn settle(&mut self) {
        if self.crossfade() >= 128 {
            self.shuffle.advance();
        }
        self.restart_hold();
    }

    pub fn restart_hold(&mut self) {
        self.elapsed = 0;
    }
    pub fn natural(&self) -> &[u32] {
        &self.natural[self.shuffle.current()]
    }
    pub fn tinted(&self) -> &[u32] {
        &self.tinted[self.shuffle.current()]
    }

    pub fn render(&self, out: &mut [u32]) {
        let weight = self.crossfade();
        for ((dst, a), b) in out
            .iter_mut()
            .zip(self.natural())
            .zip(&self.natural[self.shuffle.next()])
        {
            *dst = mix(*a, *b, weight);
        }
    }
}

fn decode(jpeg: &[u8], width: usize, height: usize) -> Result<Vec<u32>, String> {
    let mut decoder = Decoder::new(Cursor::new(jpeg));
    decoder.set_max_decoding_buffer_size(4 * 1024 * 1024);
    decoder.read_info().map_err(|e| e.to_string())?;
    let info = decoder.info().ok_or("missing JPEG dimensions")?;
    let (w, h) = (usize::from(info.width), usize::from(info.height));
    if w == 0 || h == 0 || w > 1600 || h > 1600 {
        return Err("artwork dimensions exceed bounds".into());
    }
    let pixels = decoder.decode().map_err(|e| e.to_string())?;
    let channels = match info.pixel_format {
        PixelFormat::RGB24 => 3,
        PixelFormat::L8 => 1,
        _ => return Err("unsupported artwork colors".into()),
    };
    let (fit_w, fit_h) = if w * height > h * width {
        (width, (h * width / w).max(1))
    } else {
        ((w * height / h).max(1), height)
    };
    let mut output = vec![0x080d14; width * height];
    for y in 0..fit_h {
        for x in 0..fit_w {
            let src = ((y * h / fit_h) * w + x * w / fit_w) * channels;
            let color = if channels == 1 {
                u32::from(pixels[src]) * 0x010101
            } else {
                (u32::from(pixels[src]) << 16)
                    | (u32::from(pixels[src + 1]) << 8)
                    | u32::from(pixels[src + 2])
            };
            output[(y + (height - fit_h) / 2) * width + x + (width - fit_w) / 2] = color;
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_blending_and_easing_have_independent_literal_vectors() {
        assert_eq!(green(0), 0x080d14);
        assert_eq!(green(255), 0x8bcf83);
        assert_eq!(mix(0, 0xffffff, 128), 0x808080);
        assert_eq!(mix(0x123456, 0xabcdef, 0), 0x123456);
        assert_eq!(mix(0x123456, 0xabcdef, 255), 0xabcdef);
        for duration in [1000, 5000, 6000, 65532] {
            for (quarter, expected) in [(0, 0), (1, 40), (2, 128), (3, 215), (4, 255)] {
                assert_eq!(
                    fade_weight(u64::from(duration / 4) * quarter, duration),
                    expected
                );
            }
            assert_eq!(fade_weight(u64::MAX, duration), 255);
        }
        assert_eq!(fade_weight(0, 0), 255);
        let values: Vec<_> = (0..=1000).map(|t| fade_weight(t, 1000)).collect();
        assert!(values.windows(2).all(|p| p[0] <= p[1] && p[1] <= 255));
        assert_eq!(fade_weight(50, 1000), 2);
        assert_eq!(fade_weight(950, 1000), 253);
    }

    #[test]
    fn two_minute_idle_hold_then_smooth_crossfade_reuses_textures() {
        let mut art = Artwork::new(160, 120, 9).unwrap();
        let first = art.shuffle.current();
        let next = art.shuffle.next();
        let mut out = vec![0; 160 * 120];
        let ptr = out.as_ptr();
        art.advance(HOLD_MS, false);
        art.render(&mut out);
        assert_eq!(out, art.natural[first]);
        art.advance(2500, false);
        art.render(&mut out);
        for (i, p) in out.iter().enumerate() {
            assert_eq!(*p, mix(art.natural[first][i], art.natural[next][i], 128));
        }
        art.advance(2500, false);
        art.render(&mut out);
        assert_eq!(out, art.natural[next]);
        assert_ne!(first, next);
        assert_eq!(ptr, out.as_ptr());
        art.advance(u64::MAX, false);
        assert_eq!(art.elapsed, 0);
        art.advance(u64::MAX, true);
        assert_eq!(art.elapsed, 0);
    }

    #[test]
    fn shuffled_bags_replay_cover_every_image_and_never_repeat_at_boundaries() {
        let mut a = Shuffle::new(42);
        let mut b = Shuffle::new(42);
        let mut previous = None;
        for _ in 0..100 {
            let mut bag = Vec::new();
            for _ in 0..3 {
                assert_eq!(a.current(), b.current());
                assert_ne!(previous, Some(a.current()));
                previous = Some(a.current());
                bag.push(a.current());
                a.advance();
                b.advance();
            }
            bag.sort();
            assert_eq!(bag, vec![0, 1, 2]);
        }
        let sequences: std::collections::BTreeSet<_> = (0..12)
            .map(|seed| {
                let mut s = Shuffle::new(seed);
                (0..9)
                    .map(|_| {
                        let i = s.current();
                        s.advance();
                        i
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(sequences.len() > 1);
    }

    #[test]
    fn original_colors_and_green_caches_are_distinct_and_decode_failure_is_safe() {
        for (w, h) in [(160, 120), (320, 240)] {
            let art = Artwork::new(w, h, 0).unwrap();
            for i in 0..3 {
                assert_eq!(art.natural[i].len(), w * h);
                assert!(art.natural[i]
                    .iter()
                    .any(|p| ((p >> 16) & 255) > ((p >> 8) & 255)));
                assert!(art.tinted[i]
                    .iter()
                    .all(|p| ((p >> 8) & 255) >= ((p >> 16) & 255)));
                assert_ne!(art.natural[i], art.tinted[i]);
            }
        }
        assert!(decode(b"bad", 160, 120).is_err());
        assert!(Artwork::new(0, 120, 0).is_err());
    }
}
