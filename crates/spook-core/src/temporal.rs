//! Bounded timestamped frame history for delayed reflection and ghost trails.
//!
//! One ring is allocated at construction and holds at most eight unmodified
//! RGBA8 input frames within 4 MiB of pixel storage. Per-slot timestamps,
//! the write position, the valid-frame count and the clock are small fixed
//! metadata kept outside that pixel budget. `process` never allocates.

use std::fmt;

const MAX_WIDTH: usize = 640;
const MAX_HEIGHT: usize = 480;
const MAX_DELAY_MS: u16 = 600;
const SAMPLE_INTERVAL_MS: u64 = 100;
const TRAIL_OFFSET_MS: u64 = 200;
const MAX_SLOTS: usize = 8;
const MAX_STORAGE_BYTES: usize = 4 * 1024 * 1024;

/// Delay and ghost-trail settings. Zero disables the respective effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TemporalOptions {
    /// Requested reflection delay, 0..=600 ms. Short history may give less.
    pub delay_ms: u16,
    /// Blend weight of the past trail frame, 0 (off) to 255 (trail only).
    pub trail_strength: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalError {
    EmptyDimension,
    DimensionsTooLarge,
    DimensionsOverflow,
    InputLength,
    OutputTooShort,
    TimeRegression,
    DelayOutOfRange,
    AllocationFailed,
}

impl fmt::Display for TemporalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyDimension => "frame dimensions must be nonzero",
            Self::DimensionsTooLarge => "frame dimensions must not exceed 640 x 480",
            Self::DimensionsOverflow => "frame dimensions overflow",
            Self::InputLength => "input length must be exactly width * height * 4",
            Self::OutputTooShort => "output buffer is too short",
            Self::TimeRegression => "timestamp is earlier than the previous call",
            Self::DelayOutOfRange => "delay must not exceed 600 ms",
            Self::AllocationFailed => "temporal storage allocation failed",
        };
        f.write_str(message)
    }
}

impl std::error::Error for TemporalError {}

/// Fixed ring of timestamped RGBA8 frames for one frame size.
///
/// Slots are `min(8, 4 MiB / frame_bytes)`: eight at 320 x 240, three at
/// 640 x 480. Samples are taken from the unmodified input on the first valid
/// call and then at most once per 100 ms. Processed output is never stored.
pub struct TemporalHistory {
    frame_bytes: usize,
    slots: usize,
    ring: Vec<u8>,
    stamps: [u64; MAX_SLOTS],
    /// Slot that receives the next sample; the newest sample precedes it.
    next: usize,
    len: usize,
    last_now: Option<u64>,
}

impl fmt::Debug for TemporalHistory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TemporalHistory")
            .field("frame_bytes", &self.frame_bytes)
            .field("slots", &self.slots)
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}

impl TemporalHistory {
    /// Allocates the ring for `width` x `height` RGBA8 frames.
    /// Dimensions are checked before any allocation.
    pub fn new(width: usize, height: usize) -> Result<Self, TemporalError> {
        if width == 0 || height == 0 {
            return Err(TemporalError::EmptyDimension);
        }
        let pixels = width
            .checked_mul(height)
            .ok_or(TemporalError::DimensionsOverflow)?;
        let frame_bytes = pixels
            .checked_mul(4)
            .ok_or(TemporalError::DimensionsOverflow)?;
        if width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(TemporalError::DimensionsTooLarge);
        }
        // frame_bytes <= 1_228_800, so at least three slots always fit.
        let slots = (MAX_STORAGE_BYTES / frame_bytes).min(MAX_SLOTS);
        Ok(Self {
            frame_bytes,
            slots,
            ring: allocate_storage(frame_bytes * slots)?,
            stamps: [0; MAX_SLOTS],
            next: 0,
            len: 0,
            last_now: None,
        })
    }

    /// Writes opaque RGBA8 into the first `width * height * 4` bytes of
    /// `output`; any suffix is left unchanged. Errors leave history, clock,
    /// sampling position and the whole output untouched.
    pub fn process(
        &mut self,
        input: &[u8],
        output: &mut [u8],
        now_ms: u64,
        options: TemporalOptions,
    ) -> Result<(), TemporalError> {
        if input.len() != self.frame_bytes {
            return Err(TemporalError::InputLength);
        }
        if output.len() < self.frame_bytes {
            return Err(TemporalError::OutputTooShort);
        }
        if options.delay_ms > MAX_DELAY_MS {
            return Err(TemporalError::DelayOutOfRange);
        }
        if self.last_now.is_some_and(|last| now_ms < last) {
            return Err(TemporalError::TimeRegression);
        }

        self.last_now = Some(now_ms);
        // Elapsed subtraction: the clock never regresses, so no overflow.
        let due = self.len == 0 || now_ms.saturating_sub(self.newest_stamp()) >= SAMPLE_INTERVAL_MS;
        if due {
            let start = self.next * self.frame_bytes;
            self.ring[start..start + self.frame_bytes].copy_from_slice(input);
            self.stamps[self.next] = now_ms;
            self.next = (self.next + 1) % self.slots;
            self.len = (self.len + 1).min(self.slots);
        }

        let delay = u64::from(options.delay_ms);
        let base = if options.delay_ms == 0 {
            input
        } else {
            self.frame(self.select(now_ms.saturating_sub(delay)))
        };
        let (out_pixels, _) = output[..self.frame_bytes].as_chunks_mut::<4>();
        let (base_pixels, _) = base.as_chunks::<4>();
        if options.trail_strength == 0 {
            for (out, src) in out_pixels.iter_mut().zip(base_pixels) {
                *out = [src[0], src[1], src[2], 255];
            }
        } else {
            let target = now_ms.saturating_sub(delay).saturating_sub(TRAIL_OFFSET_MS);
            let (trail_pixels, _) = self.frame(self.select(target)).as_chunks::<4>();
            let weight = u32::from(options.trail_strength);
            let keep = 255 - weight;
            let mix =
                |b: u8, t: u8| ((u32::from(b) * keep + u32::from(t) * weight + 127) / 255) as u8;
            for ((out, b), t) in out_pixels.iter_mut().zip(base_pixels).zip(trail_pixels) {
                *out = [mix(b[0], t[0]), mix(b[1], t[1]), mix(b[2], t[2]), 255];
            }
        }
        Ok(())
    }

    /// Forgets all frames, the clock and the sampling position. Storage is
    /// kept; stale bytes are unreachable because the valid count is zero.
    pub fn reset(&mut self) {
        self.next = 0;
        self.len = 0;
        self.last_now = None;
        self.stamps = [0; MAX_SLOTS];
    }

    /// Allocated ring pixel storage in bytes. Excludes the fixed metadata.
    pub fn storage_bytes(&self) -> usize {
        self.ring.len()
    }

    fn newest_slot(&self) -> usize {
        (self.next + self.slots - 1) % self.slots
    }

    fn newest_stamp(&self) -> u64 {
        self.stamps[self.newest_slot()]
    }

    fn frame(&self, slot: usize) -> &[u8] {
        &self.ring[slot * self.frame_bytes..(slot + 1) * self.frame_bytes]
    }

    /// Newest slot stamped at or before `target`, else the oldest slot.
    /// Visits at most `len <= 8` slots. Requires `len >= 1`.
    fn select(&self, target: u64) -> usize {
        let mut slot = self.newest_slot();
        for age in 0..self.len {
            slot = (self.next + self.slots - 1 - age) % self.slots;
            if self.stamps[slot] <= target {
                return slot;
            }
        }
        slot
    }
}

fn allocate_storage(bytes: usize) -> Result<Vec<u8>, TemporalError> {
    let mut storage = Vec::new();
    storage
        .try_reserve_exact(bytes)
        .map_err(|_| TemporalError::AllocationFailed)?;
    storage.resize(bytes, 0);
    Ok(storage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impossible_allocation_is_reported_without_exhausting_memory() {
        assert_eq!(
            allocate_storage(usize::MAX),
            Err(TemporalError::AllocationFailed)
        );
    }

    const OFF: TemporalOptions = TemporalOptions {
        delay_ms: 0,
        trail_strength: 0,
    };

    fn solid(bytes: usize, value: u8) -> Vec<u8> {
        vec![value; bytes]
    }

    #[test]
    fn slot_counts_and_storage_follow_the_cap() {
        for (w, h, slots) in [(1, 1, 8), (160, 120, 8), (320, 240, 8), (640, 480, 3)] {
            let history = TemporalHistory::new(w, h).unwrap();
            assert_eq!(history.slots, slots, "{w}x{h}");
            assert_eq!(history.storage_bytes(), slots * w * h * 4);
            assert!(history.storage_bytes() <= MAX_STORAGE_BYTES);
        }
    }

    #[test]
    fn dimension_errors_are_ordered_and_checked_before_allocation() {
        let err = |w, h| TemporalHistory::new(w, h).unwrap_err();
        assert_eq!(err(0, 4), TemporalError::EmptyDimension);
        assert_eq!(err(4, 0), TemporalError::EmptyDimension);
        assert_eq!(err(usize::MAX, 2), TemporalError::DimensionsOverflow);
        assert_eq!(err(usize::MAX, 1), TemporalError::DimensionsOverflow);
        assert_eq!(err(641, 1), TemporalError::DimensionsTooLarge);
        assert_eq!(err(1, 481), TemporalError::DimensionsTooLarge);
    }

    #[test]
    fn storage_is_reused_across_processing_wraparound_and_reset() {
        let mut history = TemporalHistory::new(4, 2).unwrap();
        let (ptr, cap) = (history.ring.as_ptr(), history.ring.capacity());
        let mut out = solid(32, 0);
        for i in 0..20u8 {
            let options = TemporalOptions {
                delay_ms: 300,
                trail_strength: 90,
            };
            history
                .process(&solid(32, i), &mut out, u64::from(i) * 100, options)
                .unwrap();
        }
        assert_eq!(history.len, 8);
        history.reset();
        assert_eq!(history.len, 0);
        history.process(&solid(32, 1), &mut out, 0, OFF).unwrap();
        assert_eq!((history.ring.as_ptr(), history.ring.capacity()), (ptr, cap));
        assert_eq!(history.storage_bytes(), 8 * 32);
    }

    #[test]
    fn sampling_interval_and_sparse_jumps_store_single_samples() {
        let mut history = TemporalHistory::new(2, 1).unwrap();
        let mut out = solid(8, 0);
        let mut at =
            |h: &mut TemporalHistory, t| h.process(&solid(8, 1), &mut out, t, OFF).unwrap();
        at(&mut history, 5);
        assert_eq!(history.len, 1);
        at(&mut history, 5);
        at(&mut history, 104);
        assert_eq!(history.len, 1);
        at(&mut history, 105);
        assert_eq!(history.len, 2);
        at(&mut history, 5_000_000);
        assert_eq!(history.len, 3);
        assert_eq!(history.stamps[..3], [5, 105, 5_000_000]);
    }

    #[test]
    fn errors_leave_metadata_unchanged() {
        let mut history = TemporalHistory::new(2, 1).unwrap();
        let mut out = solid(8, 0);
        history.process(&solid(8, 7), &mut out, 50, OFF).unwrap();
        let snapshot = (history.next, history.len, history.last_now, history.stamps);
        let ring = history.ring.clone();
        let bad_delay = TemporalOptions {
            delay_ms: 601,
            trail_strength: 0,
        };
        assert_eq!(
            history.process(&solid(7, 9), &mut out, 500, OFF),
            Err(TemporalError::InputLength)
        );
        assert_eq!(
            history.process(&solid(8, 9), &mut out[..7], 500, OFF),
            Err(TemporalError::OutputTooShort)
        );
        assert_eq!(
            history.process(&solid(8, 9), &mut out, 500, bad_delay),
            Err(TemporalError::DelayOutOfRange)
        );
        assert_eq!(
            history.process(&solid(8, 9), &mut out, 49, OFF),
            Err(TemporalError::TimeRegression)
        );
        assert_eq!(
            snapshot,
            (history.next, history.len, history.last_now, history.stamps)
        );
        assert_eq!(ring, history.ring);
    }
}
