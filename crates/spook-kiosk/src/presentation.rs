/// Controls how the processed frame fills the native window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FitMode {
    #[default]
    Cover,
    Contain,
    Stretch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Crop {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

const MAX_SOURCE_WIDTH: usize = 640;
const MAX_SOURCE_HEIGHT: usize = 480;
const MAX_DISPLAY_PIXELS: usize = 16 * 1024 * 1024;

/// Crops an already mirrored frame into one reusable processing-sized buffer.
pub struct Presentation {
    width: usize,
    height: usize,
    buffer: Vec<u32>,
    crop: Crop,
}

impl Presentation {
    pub fn new(width: usize, height: usize) -> Result<Self, &'static str> {
        if width == 0 || height == 0 || width > MAX_SOURCE_WIDTH || height > MAX_SOURCE_HEIGHT {
            return Err("processing dimensions must be within 1..=640 x 1..=480");
        }
        let count = width
            .checked_mul(height)
            .ok_or("processing size overflows")?;
        Ok(Self {
            width,
            height,
            buffer: vec![0; count],
            crop: Crop {
                x: 0,
                y: 0,
                width,
                height,
            },
        })
    }

    pub fn update(
        &mut self,
        pixels: &[u32],
        display_width: usize,
        display_height: usize,
        mode: FitMode,
    ) -> Result<(), &'static str> {
        if pixels.len() != self.width * self.height {
            return Err("presentation input length differs from processing dimensions");
        }
        let display_count = display_width
            .checked_mul(display_height)
            .ok_or("display size overflows")?;
        if display_width == 0 || display_height == 0 || display_count > MAX_DISPLAY_PIXELS {
            return Err("display must be nonzero and at most 16777216 pixels");
        }
        let mut width = self.width;
        let mut height = self.height;
        if mode == FitMode::Cover {
            // Wide products must also fit when this code runs on i686.
            let source_wide = (width as u64) * (display_height as u64);
            let display_wide = (display_width as u64) * (height as u64);
            if source_wide > display_wide {
                width = ((height as u64 * display_width as u64) / display_height as u64).max(1)
                    as usize;
            } else {
                height =
                    ((width as u64 * display_height as u64) / display_width as u64).max(1) as usize;
            }
        }
        let crop = Crop {
            x: (self.width - width) / 2,
            y: (self.height - height) / 2,
            width,
            height,
        };
        for y in 0..height {
            let source = (y + crop.y) * self.width + crop.x;
            self.buffer[y * width..(y + 1) * width]
                .copy_from_slice(&pixels[source..source + width]);
        }
        self.crop = crop;
        Ok(())
    }

    pub fn crop(&self) -> Crop {
        self.crop
    }

    pub fn pixels(&self) -> &[u32] {
        &self.buffer[..self.crop.width * self.crop.height]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: usize, height: usize) -> Vec<u32> {
        (0..width * height).map(|index| index as u32 + 1).collect()
    }

    #[test]
    fn wide_display_crops_rows_without_mirroring_again() {
        let mut view = Presentation::new(4, 4).unwrap();
        view.update(&frame(4, 4), 16, 8, FitMode::Cover).unwrap();
        assert_eq!(
            view.crop(),
            Crop {
                x: 0,
                y: 1,
                width: 4,
                height: 2
            }
        );
        assert_eq!(view.pixels(), &[5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn tall_display_crops_columns() {
        let mut view = Presentation::new(4, 4).unwrap();
        view.update(&frame(4, 4), 8, 16, FitMode::Cover).unwrap();
        assert_eq!(
            view.crop(),
            Crop {
                x: 1,
                y: 0,
                width: 2,
                height: 4
            }
        );
        assert_eq!(view.pixels(), &[2, 3, 6, 7, 10, 11, 14, 15]);
    }

    #[test]
    fn same_aspect_contain_and_stretch_keep_the_complete_source() {
        let source = frame(6, 4);
        let mut view = Presentation::new(6, 4).unwrap();
        for (mode, width, height) in [
            (FitMode::Cover, 12, 8),
            (FitMode::Contain, 8, 12),
            (FitMode::Stretch, 8, 12),
        ] {
            view.update(&source, width, height, mode).unwrap();
            assert_eq!(view.pixels(), source);
            assert_eq!(
                view.crop(),
                Crop {
                    x: 0,
                    y: 0,
                    width: 6,
                    height: 4
                }
            );
        }
    }

    #[test]
    fn odd_crops_and_extreme_aspects_remain_in_bounds() {
        for (width, height) in [(1, 1), (1, 7), (7, 1), (7, 5), (320, 240)] {
            let source = frame(width, height);
            let mut view = Presentation::new(width, height).unwrap();
            for (dw, dh) in [(1, 1), (1, 4096), (4096, 1), (1366, 768), (600, 1024)] {
                view.update(&source, dw, dh, FitMode::Cover).unwrap();
                let crop = view.crop();
                assert!(crop.width > 0 && crop.height > 0);
                assert!(crop.x + crop.width <= width && crop.y + crop.height <= height);
                assert!(view.pixels().iter().all(|pixel| *pixel > 0));
            }
        }
    }

    #[test]
    fn invalid_updates_preserve_output_and_crop() {
        let source = frame(4, 4);
        let mut view = Presentation::new(4, 4).unwrap();
        view.update(&source, 2, 4, FitMode::Cover).unwrap();
        let before = view.pixels().to_vec();
        let crop = view.crop();
        for (input, width, height) in [
            (&source[..15], 4, 4),
            (&source[..], 0, 4),
            (&source[..], 4, 0),
            (&source[..], usize::MAX, 2),
            (&source[..], MAX_DISPLAY_PIXELS + 1, 1),
        ] {
            assert!(view.update(input, width, height, FitMode::Cover).is_err());
            assert_eq!(view.pixels(), before);
            assert_eq!(view.crop(), crop);
        }
    }

    #[test]
    fn invalid_source_dimensions_are_rejected_before_allocation() {
        for (width, height) in [(0, 1), (1, 0), (641, 1), (1, 481), (usize::MAX, 2)] {
            assert!(Presentation::new(width, height).is_err());
        }
    }

    #[test]
    fn resizing_reuses_the_processing_sized_buffer() {
        let mut view = Presentation::new(320, 240).unwrap();
        let source = frame(320, 240);
        let pointer = view.buffer.as_ptr();
        let capacity = view.buffer.capacity();
        for (width, height) in [(1024, 600), (600, 1024), (4096, 4096), (320, 240)] {
            view.update(&source, width, height, FitMode::Cover).unwrap();
            assert_eq!(view.buffer.as_ptr(), pointer);
            assert_eq!(view.buffer.capacity(), capacity);
        }
        assert_eq!(view.buffer.len() * size_of::<u32>(), 307_200);
    }
}
