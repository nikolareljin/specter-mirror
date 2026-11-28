use crate::frame::pack_mirrored;
use spook_core::{TemporalError, TemporalHistory, TemporalOptions};

/// Generated diagnostic input and reusable native temporal output.
pub struct Sample {
    width: usize,
    height: usize,
    source: Vec<u8>,
    processed: Vec<u8>,
    pixels: Vec<u32>,
    history: TemporalHistory,
}

impl Sample {
    pub fn new(width: usize, height: usize) -> Result<Self, TemporalError> {
        let history = TemporalHistory::new(width, height)?;
        let count = width * height;
        Ok(Self {
            width,
            height,
            source: zeroed(count * 4)?,
            processed: zeroed(count * 4)?,
            pixels: zeroed(count)?,
            history,
        })
    }

    pub fn update(
        &mut self,
        now_ms: u64,
        options: TemporalOptions,
        reduced_motion: bool,
    ) -> Result<(), TemporalError> {
        let scene_ms = if reduced_motion { 0 } else { now_ms };
        let marker_width = (self.width / 8).max(1);
        let marker_x = ((scene_ms / 20) % self.width as u64) as usize;
        for (index, rgba) in self.source.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = (index % self.width, index / self.width);
            // An asymmetric moving bar makes reflection and trails visible.
            let marker = (x + self.width - marker_x) % self.width < marker_width
                && y >= self.height / 4
                && y < self.height * 3 / 4;
            *rgba = if marker {
                [114, 231, 214, 255]
            } else {
                [(x % 256) as u8 / 3, (y % 256) as u8 / 3, 32, 255]
            };
        }
        let options = if reduced_motion {
            TemporalOptions {
                delay_ms: 0,
                trail_strength: 0,
            }
        } else {
            options
        };
        self.history
            .process(&self.source, &mut self.processed, now_ms, options)?;
        pack_mirrored(&self.processed, &mut self.pixels, self.width, self.height);
        Ok(())
    }

    /// Runs unmirrored opaque RGBA from another source through the same shared
    /// history, output and mirroring buffers. The caller chooses the options;
    /// invalid input leaves history and displayed pixels unchanged.
    pub fn process_external(
        &mut self,
        rgba: &[u8],
        now_ms: u64,
        options: TemporalOptions,
    ) -> Result<(), TemporalError> {
        self.history
            .process(rgba, &mut self.processed, now_ms, options)?;
        pack_mirrored(&self.processed, &mut self.pixels, self.width, self.height);
        Ok(())
    }

    /// Forgets all temporal history and blanks the displayed pixels, so
    /// frames from a previous source cannot appear in the next one.
    pub fn reset(&mut self) {
        self.history.reset();
        self.processed.fill(0);
        self.pixels.fill(0);
    }

    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }
}

fn zeroed<T: Default + Clone>(count: usize) -> Result<Vec<T>, TemporalError> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(count)
        .map_err(|_| TemporalError::AllocationFailed)?;
    buffer.resize(count, T::default());
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAR: TemporalOptions = TemporalOptions {
        delay_ms: 0,
        trail_strength: 0,
    };

    #[test]
    fn generated_marker_moves_and_mirrors_once() {
        let mut sample = Sample::new(16, 8).unwrap();
        sample.update(0, CLEAR, false).unwrap();
        assert_eq!(sample.pixels()[2 * 16 + 15], 0x72e7d6);
        assert_ne!(sample.pixels()[2 * 16 + 1], 0x72e7d6);
        sample.update(100, CLEAR, false).unwrap();
        assert_eq!(sample.pixels()[2 * 16 + 10], 0x72e7d6);
        assert_ne!(sample.pixels()[2 * 16 + 15], 0x72e7d6);
    }

    #[test]
    fn delayed_sample_is_the_past_native_frame() {
        let mut clear = Sample::new(16, 8).unwrap();
        clear.update(0, CLEAR, false).unwrap();
        let expected = clear.pixels().to_vec();
        let mut delayed = Sample::new(16, 8).unwrap();
        let options = TemporalOptions {
            delay_ms: 250,
            trail_strength: 0,
        };
        for time in [0, 100, 200] {
            delayed.update(time, options, false).unwrap();
        }
        assert_eq!(delayed.pixels(), expected);
        clear.update(200, CLEAR, false).unwrap();
        assert_ne!(delayed.pixels(), clear.pixels());
    }

    #[test]
    fn trails_use_literal_blend_and_reduced_motion_stays_static() {
        let mut trails = Sample::new(16, 8).unwrap();
        let options = TemporalOptions {
            delay_ms: 0,
            trail_strength: 128,
        };
        trails.update(0, options, false).unwrap();
        trails.update(200, options, false).unwrap();
        // x=0: current [0,0,32], old marker [114,231,214].
        assert_eq!(trails.pixels()[2 * 16 + 15], 0x39747b);
        let mut still = Sample::new(160, 120).unwrap();
        still.update(0, options, true).unwrap();
        let expected = still.pixels().to_vec();
        still.update(12345, options, true).unwrap();
        assert_eq!(still.pixels(), expected);
    }

    #[test]
    fn buffers_are_reused_and_invalid_time_preserves_output() {
        for (w, h) in [(160, 120), (320, 240)] {
            let mut sample = Sample::new(w, h).unwrap();
            let pointers = (
                sample.source.as_ptr(),
                sample.processed.as_ptr(),
                sample.pixels.as_ptr(),
            );
            for time in 0..20 {
                sample.update(time * 100, CLEAR, false).unwrap();
            }
            let expected = sample.pixels().to_vec();
            assert_eq!(
                sample.update(0, CLEAR, false),
                Err(TemporalError::TimeRegression)
            );
            assert_eq!(sample.pixels(), expected);
            assert_eq!(
                pointers,
                (
                    sample.source.as_ptr(),
                    sample.processed.as_ptr(),
                    sample.pixels.as_ptr()
                )
            );
        }
        assert!(matches!(
            Sample::new(0, 120),
            Err(TemporalError::EmptyDimension)
        ));
        assert!(matches!(
            Sample::new(usize::MAX, 2),
            Err(TemporalError::DimensionsOverflow)
        ));
    }
}
