//! Newest-frame exchange between one capture worker and the display thread.
//!
//! Storage is three equally sized byte buffers that are allocated once:
//! the producer's back buffer, the shared front slot and the consumer's
//! buffer. Publishing and taking swap `Vec` pointers under a short lock; no
//! frame bytes are copied or allocated under the lock and no queue exists. A
//! frame published before the previous one was taken replaces it. The lock is
//! never held across a camera wait or frame processing.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// Frames older than this at consumption are dropped (500 ms is still fresh).
pub const MAX_AGE_MS: u64 = 500;
/// Number of equally sized software raw-frame buffers per capture session.
pub const SOFTWARE_BUFFERS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameMeta {
    /// Driver sequence number. It advances modulo 2^32.
    pub sequence: u32,
    /// Process-epoch time at which the frame was dequeued. This is when the
    /// application received it, not the exposure time.
    pub dequeued_ms: u64,
    /// Driver timestamp in microseconds, used only to reject regression. It
    /// is never converted to the process epoch. `None` when the driver gives
    /// none (zero).
    pub driver_us: Option<u64>,
    /// Valid payload bytes at the start of the buffer.
    pub used: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishError {
    /// Buffer length differs from the exchange, or `used` exceeds it.
    Length,
    /// Sequence, driver timestamp or dequeue time did not advance.
    Regressed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// No unread frame.
    Nothing,
    /// An unread frame older than [`MAX_AGE_MS`] was discarded.
    Stale { age_ms: u64 },
    /// The consumer buffer now holds this frame.
    Frame(FrameMeta),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub published: u64,
    pub overwritten: u64,
    pub stale: u64,
}

struct Slot {
    front: Vec<u8>,
    meta: FrameMeta,
    unread: bool,
    last: Option<FrameMeta>,
    counters: Counters,
}

pub struct LatestFrame {
    len: usize,
    slot: Mutex<Slot>,
}

/// Allocates one zeroed buffer, reporting reservation failure.
pub fn allocate(len: usize) -> Option<Vec<u8>> {
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(len).ok()?;
    buffer.resize(len, 0);
    Some(buffer)
}

impl LatestFrame {
    /// Allocates the shared slot for frames of `len` bytes.
    pub fn new(len: usize) -> Option<Self> {
        Some(Self {
            len,
            slot: Mutex::new(Slot {
                front: allocate(len)?,
                meta: FrameMeta {
                    sequence: 0,
                    dequeued_ms: 0,
                    driver_us: None,
                    used: 0,
                },
                unread: false,
                last: None,
                counters: Counters::default(),
            }),
        })
    }

    /// Total bytes of the three software raw-frame buffers.
    pub fn software_bytes(&self) -> usize {
        self.len * SOFTWARE_BUFFERS
    }

    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.slot.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Makes `back` the newest frame and returns the replaced storage in it.
    /// Rejected frames change nothing.
    pub fn publish(&self, back: &mut Vec<u8>, meta: FrameMeta) -> Result<(), PublishError> {
        if back.len() != self.len || meta.used > self.len {
            return Err(PublishError::Length);
        }
        let mut slot = self.lock();
        if let Some(last) = slot.last {
            let driver_regressed = matches!(
                (last.driver_us, meta.driver_us),
                (Some(before), Some(now)) if now <= before
            );
            let sequence_delta = meta.sequence.wrapping_sub(last.sequence);
            if sequence_delta == 0
                || sequence_delta > u32::MAX / 2
                || meta.dequeued_ms < last.dequeued_ms
                || driver_regressed
            {
                return Err(PublishError::Regressed);
            }
        }
        std::mem::swap(&mut slot.front, back);
        if slot.unread {
            slot.counters.overwritten += 1;
        }
        slot.unread = true;
        slot.meta = meta;
        slot.last = Some(meta);
        slot.counters.published += 1;
        Ok(())
    }

    /// Hands the newest unread frame to the consumer buffer, if it is fresh.
    /// An unread frame is consumed once, whether it was fresh or stale.
    pub fn take(&self, now_ms: u64, consumer: &mut Vec<u8>) -> Result<Taken, PublishError> {
        if consumer.len() != self.len {
            return Err(PublishError::Length);
        }
        let mut slot = self.lock();
        if !slot.unread {
            return Ok(Taken::Nothing);
        }
        slot.unread = false;
        let age_ms = now_ms.saturating_sub(slot.meta.dequeued_ms);
        if age_ms > MAX_AGE_MS {
            slot.counters.stale += 1;
            return Ok(Taken::Stale { age_ms });
        }
        std::mem::swap(&mut slot.front, consumer);
        Ok(Taken::Frame(slot.meta))
    }

    /// Dequeue time of the newest complete frame published so far.
    pub fn last_published_ms(&self) -> Option<u64> {
        self.lock().last.map(|meta| meta.dequeued_ms)
    }

    #[cfg(test)]
    pub fn counters(&self) -> Counters {
        self.lock().counters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(sequence: u32, dequeued_ms: u64, driver_us: Option<u64>, used: usize) -> FrameMeta {
        FrameMeta {
            sequence,
            dequeued_ms,
            driver_us,
            used,
        }
    }

    fn fixture() -> (LatestFrame, Vec<u8>, Vec<u8>) {
        (
            LatestFrame::new(8).unwrap(),
            allocate(8).unwrap(),
            allocate(8).unwrap(),
        )
    }

    #[test]
    fn newest_frame_replaces_unread_older_frames() {
        let (exchange, mut back, mut consumer) = fixture();
        for sequence in 1..=5u8 {
            back.fill(sequence);
            exchange
                .publish(&mut back, meta(u32::from(sequence), 100, None, 8))
                .unwrap();
        }
        let taken = exchange.take(100, &mut consumer).unwrap();
        assert_eq!(taken, Taken::Frame(meta(5, 100, None, 8)));
        assert_eq!(consumer, vec![5; 8]);
        assert_eq!(exchange.counters().overwritten, 4);
        assert_eq!(exchange.take(100, &mut consumer), Ok(Taken::Nothing));
    }

    #[test]
    fn exchange_swaps_the_same_three_allocations_forever() {
        let (exchange, mut back, mut consumer) = fixture();
        let mut pointers = vec![
            back.as_ptr() as usize,
            consumer.as_ptr() as usize,
            exchange.lock().front.as_ptr() as usize,
        ];
        pointers.sort_unstable();
        for sequence in 1..=1000u32 {
            exchange
                .publish(&mut back, meta(sequence, u64::from(sequence), None, 8))
                .unwrap();
            if sequence % 3 == 0 {
                exchange.take(u64::from(sequence), &mut consumer).unwrap();
            }
        }
        let mut after = vec![
            back.as_ptr() as usize,
            consumer.as_ptr() as usize,
            exchange.lock().front.as_ptr() as usize,
        ];
        after.sort_unstable();
        assert_eq!(pointers, after);
        assert_eq!(exchange.software_bytes(), 24);
        assert_eq!((back.len(), consumer.len()), (8, 8));
        assert_eq!(exchange.counters().published, 1000);
    }

    #[test]
    fn age_limit_keeps_500_ms_and_drops_501_ms() {
        for (now, expect_frame) in [(600, true), (601, false)] {
            let (exchange, mut back, mut consumer) = fixture();
            back.fill(7);
            exchange.publish(&mut back, meta(1, 100, None, 8)).unwrap();
            let taken = exchange.take(now, &mut consumer).unwrap();
            if expect_frame {
                assert_eq!(taken, Taken::Frame(meta(1, 100, None, 8)));
                assert_eq!(consumer, vec![7; 8]);
            } else {
                assert_eq!(taken, Taken::Stale { age_ms: 501 });
                assert_eq!(consumer, vec![0; 8]);
                assert_eq!(exchange.counters().stale, 1);
            }
            // Either way the frame is consumed once; display ticks cannot reuse it.
            assert_eq!(exchange.take(now, &mut consumer), Ok(Taken::Nothing));
        }
    }

    #[test]
    fn regressing_or_repeated_metadata_is_rejected_without_change() {
        let (exchange, mut back, mut consumer) = fixture();
        back.fill(1);
        exchange
            .publish(&mut back, meta(10, 100, Some(5000), 8))
            .unwrap();
        for bad in [
            meta(10, 101, Some(5001), 8), // equal sequence
            meta(9, 101, Some(5001), 8),  // regressing sequence
            meta(11, 99, Some(5001), 8),  // regressing dequeue time
            meta(11, 101, Some(5000), 8), // equal driver timestamp
            meta(11, 101, Some(4999), 8), // regressing driver timestamp
        ] {
            back.fill(9);
            assert_eq!(
                exchange.publish(&mut back, bad),
                Err(PublishError::Regressed),
                "{bad:?}"
            );
            assert_eq!(back, vec![9; 8]);
        }
        assert_eq!(exchange.counters().published, 1);
        // Equal dequeue time (clock resolution) and a missing driver stamp pass.
        back.fill(2);
        exchange.publish(&mut back, meta(11, 100, None, 8)).unwrap();
        assert_eq!(
            exchange.take(100, &mut consumer),
            Ok(Taken::Frame(meta(11, 100, None, 8)))
        );
        assert_eq!(consumer, vec![2; 8]);
    }

    #[test]
    fn sequence_wrap_is_forward_progress_but_old_frames_are_rejected() {
        let (exchange, mut back, mut consumer) = fixture();
        for (sequence, time) in [(u32::MAX - 1, 10), (u32::MAX, 11), (0, 12), (1, 13)] {
            exchange
                .publish(&mut back, meta(sequence, time, None, 8))
                .unwrap();
        }
        assert_eq!(
            exchange.publish(&mut back, meta(u32::MAX, 14, None, 8)),
            Err(PublishError::Regressed)
        );
        assert_eq!(
            exchange.take(14, &mut consumer),
            Ok(Taken::Frame(meta(1, 13, None, 8)))
        );
    }

    #[test]
    fn wrong_buffer_lengths_are_rejected() {
        let (exchange, mut back, mut consumer) = fixture();
        assert_eq!(
            exchange.publish(&mut back, meta(1, 0, None, 9)),
            Err(PublishError::Length)
        );
        let mut short = allocate(7).unwrap();
        assert_eq!(
            exchange.publish(&mut short, meta(1, 0, None, 7)),
            Err(PublishError::Length)
        );
        assert_eq!(exchange.take(0, &mut short), Err(PublishError::Length));
        assert_eq!(exchange.take(0, &mut consumer), Ok(Taken::Nothing));
        assert_eq!(exchange.last_published_ms(), None);
    }

    #[test]
    fn producer_thread_cannot_grow_storage_while_consumer_is_idle() {
        let exchange = std::sync::Arc::new(LatestFrame::new(16).unwrap());
        let producer = {
            let exchange = std::sync::Arc::clone(&exchange);
            std::thread::spawn(move || {
                let mut back = allocate(16).unwrap();
                for sequence in 1..=10_000u32 {
                    back.fill(sequence as u8);
                    exchange
                        .publish(&mut back, meta(sequence, u64::from(sequence), None, 16))
                        .unwrap();
                }
            })
        };
        producer.join().unwrap();
        let mut consumer = allocate(16).unwrap();
        let taken = exchange.take(10_000, &mut consumer).unwrap();
        assert_eq!(taken, Taken::Frame(meta(10_000, 10_000, None, 16)));
        assert_eq!(consumer, vec![10_000u32 as u8; 16]);
        assert_eq!(exchange.counters().overwritten, 9_999);
        assert_eq!(exchange.software_bytes(), 48);
    }
}
