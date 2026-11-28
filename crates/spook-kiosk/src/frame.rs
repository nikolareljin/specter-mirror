//! Raw camera frame layout, conversion to processing-sized RGBA and the shared
//! native pixel packing.
//!
//! Conversion pipeline (CPU only, integer only):
//! 1. A [`RawLayout`] validates the negotiated width, height, row stride and
//!    buffer size once. A raw frame is then checked against it with
//!    [`RawLayout::check_used`] before any output byte changes.
//! 2. The source is center-cropped to the largest 4:3 rectangle (floor, no
//!    stretching). A source smaller than the processing size is rejected rather
//!    than upscaled.
//! 3. Each target pixel is the rounded box average of the source block
//!    `[x0 + dx*cw/tw, x0 + (dx+1)*cw/tw)` x the same for rows. YUYV samples are
//!    converted per source pixel first, so chroma is not averaged separately.
//! 4. The unmirrored RGBA result feeds the shared temporal history. Native
//!    pixels are packed with exactly one horizontal reflection.

use std::fmt;

/// Largest raw frame buffer accepted at negotiation, including row padding.
pub const MAX_RAW_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_WIDTH: usize = 640;
pub const MAX_HEIGHT: usize = 480;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// V4L2 `RGB3`: bytes R, G, B per pixel.
    Rgb24,
    /// V4L2 `YUYV`: bytes Y0, U, Y1, V per two pixels.
    Yuyv,
}

impl PixelFormat {
    pub const fn fourcc(self) -> [u8; 4] {
        match self {
            Self::Rgb24 => *b"RGB3",
            Self::Yuyv => *b"YUYV",
        }
    }

    pub fn from_fourcc(code: [u8; 4]) -> Option<Self> {
        [Self::Rgb24, Self::Yuyv]
            .into_iter()
            .find(|format| format.fourcc() == code)
    }

    const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgb24 => 3,
            Self::Yuyv => 2,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Rgb24 => "RGB24",
            Self::Yuyv => "YUYV",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    EmptyDimension,
    TooLarge,
    Overflow,
    OddYuyvWidth,
    BadStride,
    BadBufferSize,
    ShortFrame,
    SourceTooSmall,
    AllocationFailed,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyDimension => "frame dimensions must be nonzero",
            Self::TooLarge => "frame exceeds 640 x 480 or 4 MiB",
            Self::Overflow => "frame dimensions overflow",
            Self::OddYuyvWidth => "YUYV width must be even",
            Self::BadStride => "row stride is shorter than one row",
            Self::BadBufferSize => "buffer size is smaller than stride * height",
            Self::ShortFrame => "frame payload is shorter than the negotiated rows",
            Self::SourceTooSmall => "source is smaller than the processing size",
            Self::AllocationFailed => "frame buffer allocation failed",
        })
    }
}

impl std::error::Error for FrameError {}

/// Validated geometry of one negotiated raw frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawLayout {
    pub format: PixelFormat,
    pub width: usize,
    pub height: usize,
    /// Bytes from the start of one row to the next, including padding.
    pub stride: usize,
    /// Size of one software raw-frame buffer, from the driver's image size.
    pub buffer_len: usize,
    /// Smallest payload holding every row: `(height - 1) * stride + row`.
    pub min_len: usize,
}

impl RawLayout {
    pub fn new(
        format: PixelFormat,
        width: usize,
        height: usize,
        stride: usize,
        size_image: usize,
    ) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::EmptyDimension);
        }
        if width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(FrameError::TooLarge);
        }
        if format == PixelFormat::Yuyv && !width.is_multiple_of(2) {
            return Err(FrameError::OddYuyvWidth);
        }
        let row = width
            .checked_mul(format.bytes_per_pixel())
            .ok_or(FrameError::Overflow)?;
        if stride < row {
            return Err(FrameError::BadStride);
        }
        let full = stride.checked_mul(height).ok_or(FrameError::Overflow)?;
        if size_image < full {
            return Err(FrameError::BadBufferSize);
        }
        if size_image > MAX_RAW_BYTES {
            return Err(FrameError::TooLarge);
        }
        Ok(Self {
            format,
            width,
            height,
            stride,
            buffer_len: size_image,
            min_len: (height - 1) * stride + row,
        })
    }

    /// Checks a delivered payload length against the layout and the buffer.
    pub fn check_used(&self, used: usize, available: usize) -> Result<(), FrameError> {
        if used > available || used > self.buffer_len {
            return Err(FrameError::BadBufferSize);
        }
        if used < self.min_len {
            return Err(FrameError::ShortFrame);
        }
        Ok(())
    }
}

/// BT.601 limited-range YCbCr to RGB, integer arithmetic.
///
/// With `c = y - 16`, `d = u - 128`, `e = v - 128`:
/// `r = (298c + 409e + 128) >> 8`,
/// `g = (298c - 100d - 208e + 128) >> 8`,
/// `b = (298c + 516d + 128) >> 8`, each clamped to 0..=255.
/// Arithmetic shifts floor toward negative infinity before clamping.
pub fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let c = i32::from(y) - 16;
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let clamp = |value: i32| (value >> 8).clamp(0, 255) as u8;
    [
        clamp(298 * c + 409 * e + 128),
        clamp(298 * c - 100 * d - 208 * e + 128),
        clamp(298 * c + 516 * d + 128),
    ]
}

/// Reusable crop/resize/convert stage producing unmirrored opaque RGBA.
pub struct Converter {
    layout: RawLayout,
    width: usize,
    height: usize,
    crop_x: usize,
    crop_y: usize,
    crop_width: usize,
    crop_height: usize,
    rgba: Vec<u8>,
}

impl Converter {
    pub fn new(layout: RawLayout, width: usize, height: usize) -> Result<Self, FrameError> {
        if width == 0 || height == 0 {
            return Err(FrameError::EmptyDimension);
        }
        if width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(FrameError::TooLarge);
        }
        // Largest 4:3 rectangle inside the source, centered (floor).
        let (crop_width, crop_height) = if layout.width * 3 >= layout.height * 4 {
            (layout.height * 4 / 3, layout.height)
        } else {
            (layout.width, layout.width * 3 / 4)
        };
        if crop_width < width || crop_height < height {
            return Err(FrameError::SourceTooSmall);
        }
        let count = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or(FrameError::Overflow)?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(count)
            .map_err(|_| FrameError::AllocationFailed)?;
        rgba.resize(count, 0);
        Ok(Self {
            layout,
            width,
            height,
            crop_x: (layout.width - crop_width) / 2,
            crop_y: (layout.height - crop_height) / 2,
            crop_width,
            crop_height,
            rgba,
        })
    }

    /// Crop rectangle `(x, y, width, height)` in source pixels.
    #[cfg(test)]
    pub fn crop(&self) -> (usize, usize, usize, usize) {
        (self.crop_x, self.crop_y, self.crop_width, self.crop_height)
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Converts the first `used` bytes of `raw`. Validation happens before the
    /// first output write, so a rejected frame leaves the output unchanged.
    pub fn convert(&mut self, raw: &[u8], used: usize) -> Result<&[u8], FrameError> {
        self.layout.check_used(used, raw.len())?;
        let raw = &raw[..used];
        for dy in 0..self.height {
            let y_start = self.crop_y + dy * self.crop_height / self.height;
            let y_end = self.crop_y + (dy + 1) * self.crop_height / self.height;
            for dx in 0..self.width {
                let x_start = self.crop_x + dx * self.crop_width / self.width;
                let x_end = self.crop_x + (dx + 1) * self.crop_width / self.width;
                let mut sum = [0u32; 3];
                for y in y_start..y_end {
                    for x in x_start..x_end {
                        let rgb = self.source_pixel(raw, x, y);
                        sum[0] += u32::from(rgb[0]);
                        sum[1] += u32::from(rgb[1]);
                        sum[2] += u32::from(rgb[2]);
                    }
                }
                let count = ((x_end - x_start) * (y_end - y_start)) as u32;
                let average = |value: u32| ((value + count / 2) / count) as u8;
                let out = (dy * self.width + dx) * 4;
                self.rgba[out..out + 4].copy_from_slice(&[
                    average(sum[0]),
                    average(sum[1]),
                    average(sum[2]),
                    255,
                ]);
            }
        }
        Ok(&self.rgba)
    }

    fn source_pixel(&self, raw: &[u8], x: usize, y: usize) -> [u8; 3] {
        let row = y * self.layout.stride;
        match self.layout.format {
            PixelFormat::Rgb24 => {
                let at = row + x * 3;
                [raw[at], raw[at + 1], raw[at + 2]]
            }
            PixelFormat::Yuyv => {
                let pair = row + (x / 2) * 4;
                yuv_to_rgb(raw[pair + (x % 2) * 2], raw[pair + 1], raw[pair + 3])
            }
        }
    }
}

/// Packs opaque RGBA into `0x00RRGGBB` with exactly one horizontal reflection.
pub fn pack_mirrored(rgba: &[u8], pixels: &mut [u32], width: usize, height: usize) {
    for y in 0..height {
        for x in 0..width {
            let source = (y * width + width - 1 - x) * 4;
            let rgba = &rgba[source..source + 4];
            pixels[y * width + x] =
                (u32::from(rgba[0]) << 16) | (u32::from(rgba[1]) << 8) | u32::from(rgba[2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb_layout(width: usize, height: usize, stride: usize) -> RawLayout {
        RawLayout::new(PixelFormat::Rgb24, width, height, stride, stride * height).unwrap()
    }

    fn yuyv_layout(width: usize, height: usize, stride: usize) -> RawLayout {
        RawLayout::new(PixelFormat::Yuyv, width, height, stride, stride * height).unwrap()
    }

    #[test]
    fn layout_rejects_invalid_geometry() {
        let new = RawLayout::new;
        let (rgb, yuyv) = (PixelFormat::Rgb24, PixelFormat::Yuyv);
        assert_eq!(new(rgb, 0, 4, 12, 48), Err(FrameError::EmptyDimension));
        assert_eq!(new(rgb, 4, 0, 12, 48), Err(FrameError::EmptyDimension));
        assert_eq!(new(yuyv, 5, 4, 10, 40), Err(FrameError::OddYuyvWidth));
        assert_eq!(new(rgb, 641, 4, 1923, 9000), Err(FrameError::TooLarge));
        assert_eq!(new(rgb, 4, 481, 12, 6000), Err(FrameError::TooLarge));
        assert_eq!(new(rgb, 4, 4, 11, 48), Err(FrameError::BadStride));
        assert_eq!(new(rgb, 4, 4, 12, 47), Err(FrameError::BadBufferSize));
        assert_eq!(
            new(rgb, 4, 4, usize::MAX, usize::MAX),
            Err(FrameError::Overflow)
        );
        // 640 x 480 with a stride that pushes the buffer past 4 MiB.
        assert_eq!(
            new(rgb, 640, 480, 8800, 8800 * 480),
            Err(FrameError::TooLarge)
        );
        let ok = new(yuyv, 640, 480, 1280, 1280 * 480).unwrap();
        assert_eq!((ok.min_len, ok.buffer_len), (1280 * 479 + 1280, 614_400));
    }

    #[test]
    fn payload_length_is_checked_against_rows_and_buffer() {
        let layout = rgb_layout(4, 2, 16); // min_len = 16 + 12
        assert_eq!(layout.min_len, 28);
        assert_eq!(layout.check_used(28, 32), Ok(()));
        assert_eq!(layout.check_used(32, 32), Ok(()));
        assert_eq!(layout.check_used(27, 32), Err(FrameError::ShortFrame));
        assert_eq!(layout.check_used(33, 40), Err(FrameError::BadBufferSize));
        assert_eq!(layout.check_used(30, 29), Err(FrameError::BadBufferSize));
    }

    #[test]
    fn yuyv_literal_colors_and_clamps() {
        for (yuv, rgb) in [
            ((16, 128, 128), [0, 0, 0]),
            ((235, 128, 128), [255, 255, 255]),
            ((128, 128, 128), [130, 130, 130]),
            ((81, 90, 240), [255, 0, 0]),
            ((145, 54, 34), [0, 255, 1]),
            ((41, 240, 110), [0, 0, 255]),
            // Boundaries: below black, above white, saturated chroma.
            ((0, 128, 128), [0, 0, 0]),
            ((15, 128, 128), [0, 0, 0]),
            ((17, 128, 128), [1, 1, 1]),
            ((255, 128, 128), [255, 255, 255]),
            ((16, 0, 255), [203, 0, 0]),
            ((16, 255, 0), [0, 54, 255]),
            ((235, 0, 255), [255, 202, 0]),
        ] {
            assert_eq!(yuv_to_rgb(yuv.0, yuv.1, yuv.2), rgb, "{yuv:?}");
        }
    }

    #[test]
    fn rgb24_converts_literal_pixels_and_skips_row_padding() {
        // 4 x 3 source, stride 16 (4 padding bytes of 0xEE per row), target 4 x 3.
        let layout = rgb_layout(4, 3, 16);
        let mut raw = vec![0xEE; 48];
        for y in 0..3 {
            for x in 0..4 {
                let at = y * 16 + x * 3;
                raw[at..at + 3].copy_from_slice(&[(x * 10) as u8, (y * 20) as u8, 7]);
            }
        }
        let mut converter = Converter::new(layout, 4, 3).unwrap();
        let rgba = converter.convert(&raw, 48).unwrap().to_vec();
        for y in 0..3 {
            for x in 0..4 {
                let at = (y * 4 + x) * 4;
                assert_eq!(
                    rgba[at..at + 4],
                    [(x * 10) as u8, (y * 20) as u8, 7, 255],
                    "x={x} y={y}"
                );
            }
        }
    }

    #[test]
    fn yuyv_pairs_share_chroma_and_padding_is_ignored() {
        // 4 x 3 source, stride 12 (8 pixel bytes + 4 padding bytes).
        let layout = yuyv_layout(4, 3, 12);
        let mut raw = vec![0xEE; 36];
        for y in 0..3 {
            // Pixels 0,1: red chroma; pixels 2,3: blue chroma. Y differs in a pair.
            raw[y * 12..y * 12 + 8].copy_from_slice(&[81, 90, 81, 240, 41, 240, 41, 110]);
        }
        let mut converter = Converter::new(layout, 4, 3).unwrap();
        let rgba = converter.convert(&raw, 36).unwrap();
        // First row: (Y=81,U=90,V=240) -> red twice; (Y=41,U=240,V=110) -> blue twice.
        assert_eq!(rgba[0..4], [255, 0, 0, 255]);
        assert_eq!(rgba[4..8], [255, 0, 0, 255]);
        assert_eq!(rgba[8..12], [0, 0, 255, 255]);
        assert_eq!(rgba[12..16], [0, 0, 255, 255]);
    }

    #[test]
    fn crop_is_centered_four_by_three_without_stretching() {
        // 8 x 3: wide. Crop height 3, width 4 (floor 3*4/3), x offset 2.
        let wide = Converter::new(rgb_layout(8, 3, 24), 4, 3).unwrap();
        assert_eq!(wide.crop(), (2, 0, 4, 3));
        // 6 x 6: square. Crop width 6, height 4 (floor 18/4), y offset 1.
        let square = Converter::new(rgb_layout(6, 6, 18), 6, 4).unwrap();
        assert_eq!(square.crop(), (0, 1, 6, 4));
        // 640 x 480 is already 4:3.
        let full = Converter::new(rgb_layout(640, 480, 1920), 320, 240).unwrap();
        assert_eq!(full.crop(), (0, 0, 640, 480));
        // 640 x 360 (16:9): crop 480 x 360, x offset 80.
        let wide_hd = Converter::new(rgb_layout(640, 360, 1920), 320, 240).unwrap();
        assert_eq!(wide_hd.crop(), (80, 0, 480, 360));
    }

    #[test]
    fn crop_corners_select_the_centered_source_pixels() {
        // 8 x 3 source; cropped 4 x 3 region starts at x = 2. Pixel value = x*10 + y.
        let layout = rgb_layout(8, 3, 24);
        let mut raw = vec![0u8; 72];
        for y in 0..3 {
            for x in 0..8 {
                let at = y * 24 + x * 3;
                raw[at..at + 3].copy_from_slice(&[(x * 10 + y) as u8, 0, 0]);
            }
        }
        let mut converter = Converter::new(layout, 4, 3).unwrap();
        let rgba = converter.convert(&raw, 72).unwrap();
        let red = |x: usize, y: usize| rgba[(y * 4 + x) * 4];
        assert_eq!((red(0, 0), red(3, 0)), (20, 50)); // top corners: x=2 and x=5
        assert_eq!((red(0, 2), red(3, 2)), (22, 52)); // bottom corners
    }

    #[test]
    fn downsample_is_a_rounded_box_average_at_both_sizes() {
        for (target_w, target_h, source_w, source_h) in [(160, 120, 320, 240), (320, 240, 640, 480)]
        {
            let layout = rgb_layout(source_w, source_h, source_w * 3 + 6);
            let mut raw = vec![0xEE; layout.buffer_len];
            for y in 0..source_h {
                for x in 0..source_w {
                    // Left half of every 2x2 block is 10, right half 13: mean 11.5 -> 12.
                    let value = if x % 2 == 0 { 10 } else { 13 };
                    let at = y * layout.stride + x * 3;
                    raw[at..at + 3].copy_from_slice(&[value, 0, 255]);
                }
            }
            let mut converter = Converter::new(layout, target_w, target_h).unwrap();
            let rgba = converter.convert(&raw, layout.buffer_len).unwrap();
            assert_eq!(rgba.len(), target_w * target_h * 4);
            assert!(rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| *pixel == [12, 0, 255, 255]));
        }
    }

    #[test]
    fn same_size_source_is_copied_exactly_at_both_sizes() {
        for (w, h) in [(160, 120), (320, 240)] {
            let layout = yuyv_layout(w, h, w * 2 + 8);
            let mut raw = vec![0xEE; layout.buffer_len];
            for y in 0..h {
                for pair in 0..w / 2 {
                    let at = y * layout.stride + pair * 4;
                    raw[at..at + 4].copy_from_slice(&[81, 90, 81, 240]);
                }
            }
            let mut converter = Converter::new(layout, w, h).unwrap();
            let rgba = converter.convert(&raw, layout.buffer_len).unwrap();
            assert_eq!(rgba[0..4], [255, 0, 0, 255]);
            assert_eq!(rgba[rgba.len() - 4..], [255, 0, 0, 255]);
        }
    }

    #[test]
    fn tiny_sources_are_not_upscaled_and_failures_keep_output() {
        assert_eq!(
            Converter::new(rgb_layout(160, 120, 480), 320, 240).err(),
            Some(FrameError::SourceTooSmall)
        );
        assert_eq!(
            Converter::new(rgb_layout(4, 3, 12), 0, 3).err(),
            Some(FrameError::EmptyDimension)
        );
        let layout = rgb_layout(4, 3, 12);
        let mut converter = Converter::new(layout, 4, 3).unwrap();
        let good = vec![9u8; 36];
        let before = converter.convert(&good, 36).unwrap().to_vec();
        let changed = vec![200u8; 36];
        assert_eq!(
            converter.convert(&changed, 35).err(),
            Some(FrameError::ShortFrame)
        );
        assert_eq!(
            converter.convert(&changed[..30], 36).err(),
            Some(FrameError::BadBufferSize)
        );
        assert_eq!(converter.rgba(), before);
    }

    #[test]
    fn mirroring_reflects_asymmetric_pixels_exactly_once() {
        // 3 x 2: distinct pixels.
        let rgba = [
            [1, 2, 3, 255],
            [4, 5, 6, 255],
            [7, 8, 9, 255],
            [10, 11, 12, 255],
            [13, 14, 15, 255],
            [16, 17, 18, 255],
        ]
        .concat();
        let mut pixels = [0u32; 6];
        pack_mirrored(&rgba, &mut pixels, 3, 2);
        assert_eq!(
            pixels,
            [0x070809, 0x040506, 0x010203, 0x101112, 0x0d0e0f, 0x0a0b0c]
        );
    }
}
