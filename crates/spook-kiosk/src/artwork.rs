use jpeg_decoder::{Decoder, PixelFormat};
use std::io::Cursor;

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

pub struct Artwork {
    frames: [Vec<u32>; 3],
}

impl Artwork {
    pub fn new(width: usize, height: usize) -> Result<Self, String> {
        if ![(160, 120), (320, 240)].contains(&(width, height)) {
            return Err("unsupported artwork size".into());
        }
        let mut frames = [Vec::new(), Vec::new(), Vec::new()];
        for (dst, jpeg) in frames.iter_mut().zip(IMAGES) {
            *dst = decode(jpeg, width, height)?;
        }
        Ok(Self { frames })
    }

    pub fn render(&self, now: u64, reduced: bool, out: &mut [u32]) {
        let (slide, weight) = if reduced {
            (0, 0)
        } else {
            let phase = now % 35_000;
            (
                (now / 35_000 % 3) as usize,
                if phase < 30_000 {
                    0
                } else {
                    ((phase - 30_000) * 255 / 5000) as u32
                },
            )
        };
        for ((dst, a), b) in out
            .iter_mut()
            .zip(&self.frames[slide])
            .zip(&self.frames[(slide + 1) % 3])
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
    let mut output = vec![green(0); width * height];
    for y in 0..fit_h {
        for x in 0..fit_w {
            let src = ((y * h / fit_h) * w + x * w / fit_w) * channels;
            let gray = if channels == 1 {
                pixels[src]
            } else {
                ((u32::from(pixels[src]) * 77
                    + u32::from(pixels[src + 1]) * 150
                    + u32::from(pixels[src + 2]) * 29
                    + 128)
                    >> 8) as u8
            };
            output[(y + (height - fit_h) / 2) * width + x + (width - fit_w) / 2] = green(gray);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palette_and_blend_have_literal_endpoints() {
        assert_eq!(green(0), 0x080d14);
        assert_eq!(green(255), 0x8bcf83);
        assert_eq!(mix(0, 0xffffff, 128), 0x808080);
        assert_eq!(mix(0x123456, 0xabcdef, 0), 0x123456);
        assert_eq!(mix(0x123456, 0xabcdef, 255), 0xabcdef);
    }
    #[test]
    fn licensed_frames_cycle_without_mirroring_or_reallocation() {
        for (w, h) in [(160, 120), (320, 240)] {
            let art = Artwork::new(w, h).unwrap();
            let mut out = vec![0; w * h];
            let ptr = out.as_ptr();
            art.render(0, false, &mut out);
            assert_eq!(out, art.frames[0]);
            art.render(35000, false, &mut out);
            assert_eq!(out, art.frames[1]);
            art.render(70000, false, &mut out);
            assert_eq!(out, art.frames[2]);
            art.render(105000, false, &mut out);
            assert_eq!(out, art.frames[0]);
            art.render(32500, false, &mut out);
            assert_eq!(out[100], mix(art.frames[0][100], art.frames[1][100], 127));
            art.render(u64::MAX, true, &mut out);
            assert_eq!(out, art.frames[0]);
            assert_eq!(ptr, out.as_ptr());
            assert_eq!(art.frames[1][0], green(0));
        }
        assert!(decode(b"bad", 160, 120).is_err());
    }
}
