//! Opt-in V4L2 capture: format negotiation, a bounded capture worker and the
//! display-side session that supervises it.
//!
//! One worker thread owns the device. It waits at most [`WAIT_SLICE_MS`] per
//! step, so cancellation is noticed within that slice plus the stop call. It
//! copies each complete frame into a preallocated buffer, stamps it with the
//! shared process clock at dequeue and swaps it into [`LatestFrame`]. The
//! display thread polls the session; it never waits on the camera.
//!
//! Storage bounds per session: three software raw-frame buffers of
//! `buffer_len` bytes (at most 4 MiB each; 614,400 for 640 x 480 YUYV), one
//! processing-sized RGBA buffer, and the backend's [`BACKEND_BUFFERS`]
//! memory-mapped driver buffers requested from the kernel.

use crate::frame::{
    Converter, FrameError, PixelFormat, RawLayout, MAX_HEIGHT, MAX_RAW_BYTES, MAX_WIDTH,
};
use crate::latest_frame::{allocate, FrameMeta, LatestFrame, PublishError, Taken};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// No complete frame within this time after the session starts: fall back.
pub const STARTUP_LIMIT_MS: u64 = 2000;
/// No complete frame for longer than this after the last one: fall back.
pub const STALL_LIMIT_MS: u64 = 1000;
/// Longest single device wait; also the cancellation latency bound.
pub const WAIT_SLICE_MS: u64 = 100;
/// Driver buffers requested from V4L2. The driver may adjust the count; the
/// kernel-side memory is outside the software accounting above.
pub const BACKEND_BUFFERS: u32 = 4;

/// Monotonic milliseconds since a shared process epoch.
pub trait Clock: Send + Sync {
    fn now_ms(&self) -> u64;
}

pub struct MonotonicClock(Instant);

impl MonotonicClock {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl Clock for MonotonicClock {
    fn now_ms(&self) -> u64 {
        self.0.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    Missing,
    Permission,
    Unsupported(String),
    Disconnected,
    Eof,
    Io(String),
    Frame(FrameError),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("camera device not found"),
            Self::Permission => {
                f.write_str("permission denied opening the camera (video group membership?)")
            }
            Self::Unsupported(reason) => write!(f, "unsupported camera: {reason}"),
            Self::Disconnected => f.write_str("camera disconnected"),
            Self::Eof => f.write_str("camera stream ended"),
            Self::Io(reason) => write!(f, "camera I/O error: {reason}"),
            Self::Frame(error) => write!(f, "camera format rejected: {error}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Capture formats offered by a device. Only supported FourCCs carry sizes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub fourcc: [u8; 4],
    pub sizes: Vec<(u32, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub format: PixelFormat,
    pub width: u32,
    pub height: u32,
}

/// What the driver reports after the format request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Readback {
    pub fourcc: [u8; 4],
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub size_image: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawFrame {
    pub used: usize,
    pub sequence: u32,
    pub driver_us: Option<u64>,
}

/// The device operations the worker needs. The V4L2 implementation is
/// [`V4lDevice`]; tests use a scripted fake.
pub trait CaptureDevice: Send {
    fn offers(&mut self) -> Result<Vec<Offer>, CaptureError>;
    fn negotiate(&mut self, choice: &Choice) -> Result<Readback, CaptureError>;
    fn start(&mut self) -> Result<(), CaptureError>;
    /// Waits up to `timeout`. `Ok(None)` has no usable frame. On success the payload
    /// is copied to the start of `into`.
    fn wait_frame(
        &mut self,
        timeout: Duration,
        into: &mut [u8],
    ) -> Result<Option<RawFrame>, CaptureError>;
    /// Stops streaming. Must be safe to call more than once or before start.
    fn stop(&mut self);
}

fn printable(code: [u8; 4]) -> String {
    code.iter()
        .map(|&byte| {
            if byte.is_ascii_graphic() {
                byte as char
            } else {
                '?'
            }
        })
        .collect()
}

/// Chooses the capture format.
///
/// Candidates are supported FourCCs (RGB24, YUYV) with 4:3 sizes of at least
/// the processing size and at most 640 x 480 (even width for YUYV). The exact
/// processing size wins; otherwise the smallest area; ties prefer RGB24.
pub fn select_format(offers: &[Offer], target: (usize, usize)) -> Result<Choice, CaptureError> {
    let mut best: Option<((u8, u64, u8), Choice)> = None;
    let mut supported = false;
    for offer in offers {
        let Some(format) = PixelFormat::from_fourcc(offer.fourcc) else {
            continue;
        };
        supported = true;
        for &(width, height) in &offer.sizes {
            let (w, h) = (width as usize, height as usize);
            let fits = w >= target.0
                && h >= target.1
                && w <= MAX_WIDTH
                && h <= MAX_HEIGHT
                && w * 3 == h * 4
                && !(format == PixelFormat::Yuyv && !w.is_multiple_of(2))
                && w * h * 3 <= MAX_RAW_BYTES;
            if !fits {
                continue;
            }
            let rank = (
                u8::from((w, h) != target),
                (w * h) as u64,
                u8::from(format == PixelFormat::Yuyv),
            );
            if best.as_ref().is_none_or(|(current, _)| rank < *current) {
                best = Some((
                    rank,
                    Choice {
                        format,
                        width,
                        height,
                    },
                ));
            }
        }
    }
    if let Some((_, choice)) = best {
        return Ok(choice);
    }
    if !supported {
        let mut names: Vec<String> = offers.iter().take(8).map(|o| printable(o.fourcc)).collect();
        if names.is_empty() {
            names.push("none".into());
        }
        return Err(CaptureError::Unsupported(format!(
            "no RGB24 or YUYV format offered (offered: {}); MJPEG decoding is not supported",
            names.join(", ")
        )));
    }
    Err(CaptureError::Unsupported(format!(
        "no 4:3 RGB24/YUYV size from {}x{} up to {MAX_WIDTH}x{MAX_HEIGHT}",
        target.0, target.1
    )))
}

fn to_usize(value: u32) -> Result<usize, CaptureError> {
    usize::try_from(value).map_err(|_| CaptureError::Frame(FrameError::Overflow))
}

/// Enumerates, requests, reads back and validates the capture format.
fn negotiate(
    device: &mut dyn CaptureDevice,
    target: (usize, usize),
) -> Result<(Choice, RawLayout), CaptureError> {
    let offers = device.offers()?;
    let choice = select_format(&offers, target)?;
    let readback = device.negotiate(&choice)?;
    if readback.fourcc != choice.format.fourcc()
        || readback.width != choice.width
        || readback.height != choice.height
    {
        return Err(CaptureError::Unsupported(format!(
            "driver replaced the requested {} {}x{} with {} {}x{}",
            choice.format.name(),
            choice.width,
            choice.height,
            printable(readback.fourcc),
            readback.width,
            readback.height
        )));
    }
    let layout = RawLayout::new(
        choice.format,
        to_usize(readback.width)?,
        to_usize(readback.height)?,
        to_usize(readback.stride)?,
        to_usize(readback.size_image)?,
    )
    .map_err(CaptureError::Frame)?;
    Ok((choice, layout))
}

pub type Opener = Box<dyn FnOnce() -> Result<Box<dyn CaptureDevice>, CaptureError> + Send>;

#[derive(Clone)]
struct Ready {
    latest: Arc<LatestFrame>,
    layout: RawLayout,
    summary: Arc<str>,
}

#[derive(Default)]
struct State {
    ready: Option<Ready>,
    ended: Option<String>,
}

struct Shared {
    cancel: AtomicBool,
    state: Mutex<State>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    fn end(&self, reason: String) {
        self.lock().ended.get_or_insert(reason);
    }
}

/// Stops streaming before the device is closed, on every exit path.
struct StopOnDrop(Box<dyn CaptureDevice>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.stop();
    }
}

fn worker(open: Opener, target: (usize, usize), shared: &Shared, clock: &dyn Clock) {
    if let Err(error) = run(open, target, shared, clock) {
        if !shared.cancelled() {
            shared.end(error.to_string());
        }
    }
}

fn run(
    open: Opener,
    target: (usize, usize),
    shared: &Shared,
    clock: &dyn Clock,
) -> Result<(), CaptureError> {
    let mut device = StopOnDrop(open()?);
    let (choice, layout) = negotiate(&mut *device.0, target)?;
    let alloc = || CaptureError::Frame(FrameError::AllocationFailed);
    let latest = Arc::new(LatestFrame::new(layout.buffer_len).ok_or_else(alloc)?);
    let mut back = allocate(layout.buffer_len).ok_or_else(alloc)?;
    if shared.cancelled() {
        return Ok(());
    }
    device.0.start()?;
    shared.lock().ready = Some(Ready {
        latest: Arc::clone(&latest),
        layout,
        summary: format!(
            "{} {}x{} stride {} -> {}x{}; {} software raw bytes",
            choice.format.name(),
            choice.width,
            choice.height,
            layout.stride,
            target.0,
            target.1,
            latest.software_bytes()
        )
        .into(),
    });
    let wait = Duration::from_millis(WAIT_SLICE_MS);
    while !shared.cancelled() {
        let Some(raw) = device.0.wait_frame(wait, &mut back)? else {
            continue;
        };
        let dequeued_ms = clock.now_ms();
        if layout.check_used(raw.used, back.len()).is_err() {
            // Malformed payload metadata: not a complete frame; never published.
            continue;
        }
        let meta = FrameMeta {
            sequence: raw.sequence,
            dequeued_ms,
            driver_us: raw.driver_us,
            used: raw.used,
        };
        match latest.publish(&mut back, meta) {
            Ok(()) | Err(PublishError::Regressed) => {}
            Err(PublishError::Length) => {
                return Err(CaptureError::Io("internal buffer length mismatch".into()))
            }
        }
    }
    Ok(())
}

struct Consumer {
    latest: Arc<LatestFrame>,
    buffer: Vec<u8>,
    converter: Converter,
    summary: Arc<str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CameraPoll {
    /// Nothing new; keep the displayed output.
    Idle,
    /// A new frame was converted; read it with [`CameraSession::rgba`].
    Frame { time_ms: u64 },
    /// The camera cannot be used. Cancellation has been requested.
    Fallback(String),
}

/// Display-side owner of one capture worker. Dropping it cancels and joins.
pub struct CameraSession {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    started_ms: u64,
    target: (usize, usize),
    consumer: Option<Consumer>,
    stopped: bool,
}

impl CameraSession {
    /// Starts the worker. Opening and negotiation happen on the worker, so
    /// the caller never waits on the device.
    pub fn spawn(
        open: Opener,
        target: (usize, usize),
        clock: Arc<dyn Clock>,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            cancel: AtomicBool::new(false),
            state: Mutex::new(State::default()),
        });
        let started_ms = clock.now_ms();
        let handle = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("camera-capture".into())
                .spawn(move || worker(open, target, &shared, &*clock))?
        };
        Ok(Self {
            shared,
            worker: Some(handle),
            started_ms,
            target,
            consumer: None,
            stopped: false,
        })
    }

    /// Requests cancellation. The worker stops within one wait slice.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Release);
    }

    /// Joins the worker if it has finished. Returns true when none remains.
    pub fn try_reap(&mut self) -> bool {
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            self.join();
        }
        self.worker.is_none()
    }

    fn join(&mut self) {
        if let Some(handle) = self.worker.take() {
            // A worker panic (for example a backend drop failure) is already
            // contained in the thread; the device is closed during unwinding.
            let _ = handle.join();
        }
    }

    pub fn rgba(&self) -> &[u8] {
        self.consumer
            .as_ref()
            .map_or(&[][..], |consumer| consumer.converter.rgba())
    }

    /// Negotiated format summary, once streaming has started.
    pub fn summary(&self) -> Option<&str> {
        self.consumer.as_ref().map(|c| &*c.summary)
    }

    fn fallback(&mut self, reason: String) -> CameraPoll {
        self.cancel();
        self.stopped = true;
        CameraPoll::Fallback(reason)
    }

    /// Non-blocking supervision step at process time `now_ms`.
    pub fn poll(&mut self, now_ms: u64) -> CameraPoll {
        if self.stopped {
            return CameraPoll::Idle;
        }
        let finished = self.worker.as_ref().is_none_or(JoinHandle::is_finished);
        let (ready, ended) = {
            let state = self.shared.lock();
            (
                state.ready.clone().filter(|_| self.consumer.is_none()),
                state.ended.clone(),
            )
        };
        if let Some(ready) = ready {
            match self.build_consumer(ready) {
                Ok(consumer) => self.consumer = Some(consumer),
                Err(error) => return self.fallback(error.to_string()),
            }
        }
        if let Some(reason) = ended {
            return self.fallback(reason);
        }
        if finished {
            return self.fallback("camera worker stopped unexpectedly".into());
        }
        let last = self
            .consumer
            .as_ref()
            .and_then(|consumer| consumer.latest.last_published_ms());
        match last {
            None if now_ms.saturating_sub(self.started_ms) > STARTUP_LIMIT_MS => {
                return self.fallback("no camera frame within 2 seconds of startup".into());
            }
            Some(last) if now_ms.saturating_sub(last) > STALL_LIMIT_MS => {
                return self.fallback("no camera frame for more than 1 second".into());
            }
            _ => {}
        }
        let Some(consumer) = self.consumer.as_mut() else {
            return CameraPoll::Idle;
        };
        match consumer.latest.take(now_ms, &mut consumer.buffer) {
            Ok(Taken::Frame(meta)) => {
                match consumer.converter.convert(&consumer.buffer, meta.used) {
                    Ok(_) => CameraPoll::Frame {
                        time_ms: meta.dequeued_ms,
                    },
                    Err(error) => self.fallback(format!("invalid camera frame: {error}")),
                }
            }
            Ok(Taken::Nothing | Taken::Stale { .. }) => CameraPoll::Idle,
            Err(_) => self.fallback("internal frame exchange error".into()),
        }
    }

    fn build_consumer(&self, ready: Ready) -> Result<Consumer, CaptureError> {
        let alloc = || CaptureError::Frame(FrameError::AllocationFailed);
        Ok(Consumer {
            buffer: allocate(ready.layout.buffer_len).ok_or_else(alloc)?,
            converter: Converter::new(ready.layout, self.target.0, self.target.1)
                .map_err(CaptureError::Frame)?,
            latest: ready.latest,
            summary: ready.summary,
        })
    }
}

impl Drop for CameraSession {
    fn drop(&mut self) {
        self.cancel();
        self.join();
    }
}

pub use v4l_backend::open_index;
#[cfg(test)]
pub use v4l_backend::open_path;

mod v4l_backend {
    use super::*;
    use v4l::buffer::Type;
    use v4l::capability::Flags as Caps;
    use v4l::framesize::FrameSizeEnum;
    use v4l::io::traits::{CaptureStream, Stream as _};
    use v4l::video::Capture;
    use v4l::{Device, Format, FourCC};

    const POLLIN: i16 = 0x0001; // Linux capture readiness.
    const MAX_FORMATS: usize = 32;
    const MAX_SIZES: usize = 64;
    /// Sizes tried for stepwise/continuous ranges.
    const STEPWISE_CANDIDATES: [(u32, u32); 3] = [(160, 120), (320, 240), (640, 480)];

    fn map_io(error: &std::io::Error) -> CaptureError {
        use std::io::ErrorKind;
        match error.kind() {
            ErrorKind::NotFound => CaptureError::Missing,
            ErrorKind::PermissionDenied => CaptureError::Permission,
            ErrorKind::UnexpectedEof => CaptureError::Eof,
            _ if error.raw_os_error() == Some(19) => CaptureError::Disconnected,
            kind => CaptureError::Io(kind.to_string()),
        }
    }

    trait StreamRead {
        fn ready(&mut self, timeout: Duration) -> std::io::Result<bool>;
        fn next_frame(
            &mut self,
            timeout: Duration,
            into: &mut [u8],
        ) -> std::io::Result<Option<RawFrame>>;
        fn recover_dequeue(&mut self, timeout: Duration) -> std::io::Result<()>;
    }

    struct TimedStream<S> {
        inner: S,
        pending_dequeue: bool,
        initialized: bool,
    }

    impl<S: StreamRead> TimedStream<S> {
        fn new(inner: S) -> Self {
            Self {
                inner,
                pending_dequeue: false,
                initialized: false,
            }
        }

        fn wait(
            &mut self,
            timeout: Duration,
            into: &mut [u8],
        ) -> Result<Option<RawFrame>, CaptureError> {
            let began = Instant::now();
            if self.initialized && !self.pending_dequeue {
                // Keep the dequeued buffer until another frame is ready.
                // Otherwise slow cameras can time out on every next() call.
                match self.inner.ready(timeout) {
                    Ok(false) => return Ok(None),
                    Ok(true) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::Interrupted
                                | std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::TimedOut
                        ) =>
                    {
                        return Ok(None)
                    }
                    Err(error) => return Err(map_io(&error)),
                }
            }
            let remaining = timeout.saturating_sub(began.elapsed());
            let result = if self.pending_dequeue {
                // next() already queued a buffer before timing out. Finish
                // that dequeue without queueing it a second time.
                self.inner.recover_dequeue(remaining).map(|()| None)
            } else {
                self.initialized = true;
                self.inner.next_frame(remaining, into)
            };
            match result {
                Ok(frame) => {
                    self.pending_dequeue = false;
                    Ok(frame)
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut
                            | std::io::ErrorKind::WouldBlock
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    self.pending_dequeue = true;
                    Ok(None)
                }
                Err(error) => Err(map_io(&error)),
            }
        }
    }

    // time_t is 32 bits wide on i686 and 64 bits elsewhere.
    #[allow(clippy::useless_conversion)]
    fn copy_frame(
        data: &[u8],
        meta: &v4l::buffer::Metadata,
        into: &mut [u8],
    ) -> std::io::Result<Option<RawFrame>> {
        if meta.flags.contains(v4l::buffer::Flags::ERROR) {
            return Ok(None);
        }
        let used = meta.bytesused as usize;
        if used > data.len() || used > into.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "camera payload exceeds its negotiated buffer",
            ));
        }
        let seconds = i64::from(meta.timestamp.sec);
        let microseconds = i64::from(meta.timestamp.usec);
        if seconds < 0 || !(0..1_000_000).contains(&microseconds) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid camera timestamp",
            ));
        }
        let micros = seconds
            .checked_mul(1_000_000)
            .and_then(|us| us.checked_add(microseconds))
            .and_then(|us| u64::try_from(us).ok())
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "camera timestamp overflow")
            })?;
        into[..used].copy_from_slice(&data[..used]);
        Ok(Some(RawFrame {
            used,
            sequence: meta.sequence,
            driver_us: (micros != 0).then_some(micros),
        }))
    }

    impl StreamRead for v4l::io::mmap::Stream<'_> {
        fn ready(&mut self, timeout: Duration) -> std::io::Result<bool> {
            let millis = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
            self.handle().poll(POLLIN, millis).map(|count| count != 0)
        }

        fn next_frame(
            &mut self,
            timeout: Duration,
            into: &mut [u8],
        ) -> std::io::Result<Option<RawFrame>> {
            self.set_timeout(timeout);
            let (data, meta) = CaptureStream::next(self)?;
            copy_frame(data, meta, into)
        }

        fn recover_dequeue(&mut self, timeout: Duration) -> std::io::Result<()> {
            self.set_timeout(timeout);
            // The recovery frame is discarded. next() will requeue the
            // publicly returned buffer index on the following worker step.
            CaptureStream::dequeue(self).map(|_| ())
        }
    }

    pub struct V4lDevice {
        device: Device,
        stream: Option<TimedStream<v4l::io::mmap::Stream<'static>>>,
    }

    impl V4lDevice {
        fn open(path: &str) -> Result<Self, CaptureError> {
            let device = Device::with_path(path).map_err(|error| map_io(&error))?;
            let caps = device
                .query_caps()
                .map_err(|error| map_io(&error))?
                .capabilities;
            if !caps.contains(Caps::VIDEO_CAPTURE) {
                return Err(CaptureError::Unsupported(
                    if caps.contains(Caps::VIDEO_CAPTURE_MPLANE) {
                        "multi-plane capture is not supported".into()
                    } else {
                        "not a single-plane video capture device".into()
                    },
                ));
            }
            if !caps.contains(Caps::STREAMING) {
                return Err(CaptureError::Unsupported(
                    "streaming capture is not available".into(),
                ));
            }
            Ok(Self {
                device,
                stream: None,
            })
        }
    }

    impl CaptureDevice for V4lDevice {
        fn offers(&mut self) -> Result<Vec<Offer>, CaptureError> {
            let descriptions = self.device.enum_formats().map_err(|error| map_io(&error))?;
            let mut offers = Vec::new();
            for description in descriptions.into_iter().take(MAX_FORMATS) {
                let fourcc = description.fourcc.repr;
                let mut sizes = Vec::new();
                if PixelFormat::from_fourcc(fourcc).is_some() {
                    // A driver that cannot list sizes simply offers none.
                    for entry in self
                        .device
                        .enum_framesizes(description.fourcc)
                        .unwrap_or_default()
                    {
                        match entry.size {
                            FrameSizeEnum::Discrete(size) => sizes.push((size.width, size.height)),
                            FrameSizeEnum::Stepwise(range) => {
                                let aligned = |value: u32, min: u32, step: u32| {
                                    value >= min && (value - min).is_multiple_of(step.max(1))
                                };
                                sizes.extend(STEPWISE_CANDIDATES.into_iter().filter(|&(w, h)| {
                                    w <= range.max_width
                                        && h <= range.max_height
                                        && aligned(w, range.min_width, range.step_width)
                                        && aligned(h, range.min_height, range.step_height)
                                }));
                            }
                        }
                        if sizes.len() >= MAX_SIZES {
                            sizes.truncate(MAX_SIZES);
                            break;
                        }
                    }
                }
                offers.push(Offer { fourcc, sizes });
            }
            Ok(offers)
        }

        fn negotiate(&mut self, choice: &Choice) -> Result<Readback, CaptureError> {
            let request = Format::new(
                choice.width,
                choice.height,
                FourCC::new(&choice.format.fourcc()),
            );
            self.device
                .set_format(&request)
                .map_err(|error| map_io(&error))?;
            // Read the driver's state back instead of trusting the request.
            let format = self.device.format().map_err(|error| map_io(&error))?;
            Ok(Readback {
                fourcc: format.fourcc.repr,
                width: format.width,
                height: format.height,
                stride: format.stride,
                size_image: format.size,
            })
        }

        fn start(&mut self) -> Result<(), CaptureError> {
            let stream = v4l::io::mmap::Stream::with_buffers(
                &self.device,
                Type::VideoCapture,
                BACKEND_BUFFERS,
            )
            .map_err(|error| map_io(&error))?;
            // First next() queues the actual granted buffers and starts the
            // stream through its public initialization path.
            self.stream = Some(TimedStream::new(stream));
            Ok(())
        }

        fn wait_frame(
            &mut self,
            timeout: Duration,
            into: &mut [u8],
        ) -> Result<Option<RawFrame>, CaptureError> {
            self.stream
                .as_mut()
                .ok_or_else(|| CaptureError::Io("capture is not started".into()))?
                .wait(timeout, into)
        }

        fn stop(&mut self) {
            if let Some(mut stream) = self.stream.take() {
                let _ = stream.inner.stop();
            }
        }
    }

    /// Opener for `/dev/video{index}`.
    pub fn open_index(index: u16) -> Opener {
        let path = format!("/dev/video{index}");
        Box::new(move || Ok(Box::new(V4lDevice::open(&path)?) as Box<dyn CaptureDevice>))
    }

    #[cfg(test)]
    pub fn open_path(path: &str) -> Result<Box<dyn CaptureDevice>, CaptureError> {
        Ok(Box::new(V4lDevice::open(path)?))
    }
    #[cfg(test)]
    mod stream_tests {
        use super::*;
        use std::collections::VecDeque;
        use std::io::{self, ErrorKind};

        enum Step {
            Ready(bool),
            Next(Result<bool, ErrorKind>),
            Recover(Result<(), ErrorKind>),
        }
        struct Protocol {
            steps: VecDeque<Step>,
            queued: bool,
            waits: usize,
        }
        impl StreamRead for Protocol {
            fn ready(&mut self, timeout: Duration) -> io::Result<bool> {
                assert!(timeout <= Duration::from_millis(WAIT_SLICE_MS));
                assert!(!self.queued);
                self.waits += 1;
                let Step::Ready(ready) = self.steps.pop_front().unwrap() else {
                    panic!("readiness poll was skipped");
                };
                Ok(ready)
            }
            fn next_frame(
                &mut self,
                timeout: Duration,
                into: &mut [u8],
            ) -> io::Result<Option<RawFrame>> {
                assert!(timeout <= Duration::from_millis(WAIT_SLICE_MS));
                assert!(!self.queued, "next() queued the pending buffer twice");
                self.queued = true;
                self.waits += 1;
                let Step::Next(result) = self.steps.pop_front().unwrap() else {
                    panic!("recovery dequeue was skipped");
                };
                let deliver = result.map_err(io::Error::from)?;
                self.queued = false;
                into[0] = 42;
                Ok(deliver.then_some(RawFrame {
                    used: 1,
                    sequence: 7,
                    driver_us: None,
                }))
            }
            fn recover_dequeue(&mut self, timeout: Duration) -> io::Result<()> {
                assert!(timeout <= Duration::from_millis(WAIT_SLICE_MS));
                assert!(self.queued, "dequeued without a pending read");
                self.waits += 1;
                let Step::Recover(result) = self.steps.pop_front().unwrap() else {
                    panic!("normal read was replaced by a recovery");
                };
                result.map_err(io::Error::from)?;
                self.queued = false;
                Ok(())
            }
        }
        fn fixture(steps: Vec<Step>) -> TimedStream<Protocol> {
            TimedStream::new(Protocol {
                steps: steps.into(),
                queued: false,
                waits: 0,
            })
        }

        #[test]
        fn timeouts_finish_the_pending_dequeue_before_queueing_again() {
            for transient in [
                ErrorKind::TimedOut,
                ErrorKind::Interrupted,
                ErrorKind::WouldBlock,
            ] {
                let mut stream = fixture(vec![
                    Step::Next(Err(transient)),
                    Step::Recover(Err(ErrorKind::TimedOut)),
                    Step::Recover(Ok(())),
                    Step::Ready(false),
                    Step::Ready(false),
                    Step::Ready(true),
                    Step::Next(Ok(true)),
                ]);
                let mut into = [0];
                for _ in 0..5 {
                    assert_eq!(stream.wait(Duration::from_millis(100), &mut into), Ok(None));
                    assert_eq!(into, [0]);
                }
                assert_eq!(
                    stream.wait(Duration::from_millis(100), &mut into),
                    Ok(Some(RawFrame {
                        used: 1,
                        sequence: 7,
                        driver_us: None
                    }))
                );
                assert_eq!(into, [42]);
                assert_eq!(stream.inner.waits, 7);
                assert!(stream.inner.steps.is_empty());
            }
        }

        #[test]
        fn successful_and_discarded_frames_need_no_recovery() {
            let mut stream = fixture(vec![
                Step::Next(Ok(false)),
                Step::Ready(true),
                Step::Next(Ok(true)),
            ]);
            let mut into = [0];
            assert_eq!(stream.wait(Duration::from_millis(100), &mut into), Ok(None));
            assert!(stream
                .wait(Duration::from_millis(100), &mut into)
                .unwrap()
                .is_some());
            assert_eq!(stream.inner.waits, 3);
        }

        #[test]
        fn corrupted_and_oversized_kernel_frames_preserve_the_destination() {
            let data = [1, 2, 3, 4];
            let mut into = [9; 4];
            let good = v4l::buffer::Metadata {
                bytesused: 4,
                sequence: 7,
                ..Default::default()
            };
            let bad = v4l::buffer::Metadata {
                flags: v4l::buffer::Flags::ERROR,
                ..good
            };
            assert_eq!(copy_frame(&data, &bad, &mut into).unwrap(), None);
            assert_eq!(into, [9; 4]);
            let oversized = v4l::buffer::Metadata {
                bytesused: 5,
                ..good
            };
            assert_eq!(
                copy_frame(&data, &oversized, &mut into).unwrap_err().kind(),
                ErrorKind::InvalidData
            );
            assert_eq!(into, [9; 4]);
            for timestamp in [
                v4l::timestamp::Timestamp { sec: -1, usec: 0 },
                v4l::timestamp::Timestamp { sec: 1, usec: -1 },
                v4l::timestamp::Timestamp {
                    sec: 1,
                    usec: 1_000_000,
                },
            ] {
                let invalid = v4l::buffer::Metadata { timestamp, ..good };
                assert!(copy_frame(&data, &invalid, &mut into).is_err());
                assert_eq!(into, [9; 4]);
            }
            let mut short = [9; 3];
            assert!(copy_frame(&data, &good, &mut short).is_err());
            assert_eq!(short, [9; 3]);
            assert_eq!(
                copy_frame(&data, &good, &mut into).unwrap(),
                Some(RawFrame {
                    used: 4,
                    sequence: 7,
                    driver_us: None
                })
            );
            assert_eq!(into, data);
        }

        #[test]
        fn terminal_recovery_error_is_reported_to_fallback() {
            let mut stream = fixture(vec![
                Step::Next(Err(ErrorKind::TimedOut)),
                Step::Recover(Err(ErrorKind::UnexpectedEof)),
            ]);
            let mut into = [0];
            assert_eq!(stream.wait(Duration::from_millis(100), &mut into), Ok(None));
            assert_eq!(
                stream.wait(Duration::from_millis(100), &mut into),
                Err(CaptureError::Eof)
            );
        }
    }
}

#[cfg(test)]
pub mod testing {
    //! Scripted capture device and manual clock for deterministic tests.

    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicU64, AtomicUsize};

    #[derive(Default)]
    pub struct ManualClock(AtomicU64);

    impl ManualClock {
        pub fn set(&self, ms: u64) {
            self.0.store(ms, Ordering::SeqCst);
        }
    }

    impl Clock for ManualClock {
        fn now_ms(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    #[derive(Clone, Debug)]
    pub enum Step {
        /// Deliver a frame whose payload bytes are all `fill`.
        Frame {
            at_ms: u64,
            used: usize,
            sequence: u32,
            driver_us: Option<u64>,
            fill: u8,
        },
        /// Deliver exactly these payload bytes.
        Data {
            at_ms: u64,
            sequence: u32,
            bytes: Vec<u8>,
        },
        Fail(CaptureError),
    }

    #[derive(Default)]
    pub struct Log {
        pub opened: AtomicUsize,
        pub started: AtomicUsize,
        pub stops: AtomicUsize,
        pub dropped: AtomicUsize,
        pub waits: AtomicUsize,
    }

    pub struct FakeDevice {
        pub offers: Result<Vec<Offer>, CaptureError>,
        pub readback: Result<Readback, CaptureError>,
        pub start: Result<(), CaptureError>,
        pub script: VecDeque<Step>,
        /// After the script: end of stream (`true`) or endless timeouts.
        pub eof_at_end: bool,
        pub clock: Arc<ManualClock>,
        pub log: Arc<Log>,
        /// When set, one scripted frame is released per permit.
        pub gate: Option<Arc<AtomicUsize>>,
    }

    impl FakeDevice {
        pub fn yuyv(width: u32, height: u32, clock: &Arc<ManualClock>, log: &Arc<Log>) -> Self {
            Self::with_format(*b"YUYV", width, height, width * 2, clock, log)
        }

        pub fn with_format(
            fourcc: [u8; 4],
            width: u32,
            height: u32,
            stride: u32,
            clock: &Arc<ManualClock>,
            log: &Arc<Log>,
        ) -> Self {
            Self {
                offers: Ok(vec![Offer {
                    fourcc,
                    sizes: vec![(width, height)],
                }]),
                readback: Ok(Readback {
                    fourcc,
                    width,
                    height,
                    stride,
                    size_image: stride * height,
                }),
                start: Ok(()),
                script: VecDeque::new(),
                eof_at_end: false,
                clock: Arc::clone(clock),
                log: Arc::clone(log),
                gate: None,
            }
        }

        pub fn open(self) -> Opener {
            self.log.opened.fetch_add(1, Ordering::SeqCst);
            Box::new(move || Ok(Box::new(self) as Box<dyn CaptureDevice>))
        }
    }

    impl Drop for FakeDevice {
        fn drop(&mut self) {
            self.log.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl CaptureDevice for FakeDevice {
        fn offers(&mut self) -> Result<Vec<Offer>, CaptureError> {
            self.offers.clone()
        }

        fn negotiate(&mut self, _choice: &Choice) -> Result<Readback, CaptureError> {
            self.readback.clone()
        }

        fn start(&mut self) -> Result<(), CaptureError> {
            self.log.started.fetch_add(1, Ordering::SeqCst);
            self.start.clone()
        }

        fn wait_frame(
            &mut self,
            _timeout: Duration,
            into: &mut [u8],
        ) -> Result<Option<RawFrame>, CaptureError> {
            self.log.waits.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                let permit = gate
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
                    .is_ok();
                if !permit {
                    std::thread::sleep(Duration::from_millis(1));
                    return Ok(None);
                }
            }
            match self.script.pop_front() {
                Some(Step::Data {
                    at_ms,
                    sequence,
                    bytes,
                }) => {
                    self.clock.set(at_ms);
                    let count = bytes.len().min(into.len());
                    into[..count].copy_from_slice(&bytes[..count]);
                    Ok(Some(RawFrame {
                        used: bytes.len(),
                        sequence,
                        driver_us: None,
                    }))
                }
                Some(Step::Frame {
                    at_ms,
                    used,
                    sequence,
                    driver_us,
                    fill,
                }) => {
                    self.clock.set(at_ms);
                    let count = used.min(into.len());
                    into[..count].fill(fill);
                    Ok(Some(RawFrame {
                        used,
                        sequence,
                        driver_us,
                    }))
                }
                Some(Step::Fail(error)) => Err(error),
                None if self.eof_at_end => Err(CaptureError::Eof),
                None => {
                    // A real wait is bounded; the cancel flag is rechecked after it.
                    std::thread::sleep(Duration::from_millis(1));
                    Ok(None)
                }
            }
        }

        fn stop(&mut self) {
            self.log.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Waits until the worker has recorded its end (device already closed).
    pub fn wait_ended(camera: &CameraSession) {
        wait_until(|| camera.shared.lock().ended.is_some());
    }

    /// Spins (bounded by wall time) until `condition` holds.
    pub fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !condition() {
            assert!(Instant::now() < deadline, "condition not reached");
            std::thread::yield_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use std::sync::atomic::Ordering::SeqCst;

    const TARGET: (usize, usize) = (320, 240);

    fn offer(code: &[u8; 4], sizes: &[(u32, u32)]) -> Offer {
        Offer {
            fourcc: *code,
            sizes: sizes.to_vec(),
        }
    }

    fn frame(at_ms: u64, sequence: u32, fill: u8) -> Step {
        Step::Frame {
            at_ms,
            used: 640 * 2 * 480,
            sequence,
            driver_us: Some(1_000 + u64::from(sequence)),
            fill,
        }
    }

    fn session(
        device: FakeDevice,
        target: (usize, usize),
        clock: &Arc<ManualClock>,
    ) -> CameraSession {
        CameraSession::spawn(device.open(), target, Arc::clone(clock) as Arc<dyn Clock>).unwrap()
    }

    fn fixture() -> (Arc<ManualClock>, Arc<Log>) {
        (Arc::new(ManualClock::default()), Arc::new(Log::default()))
    }

    #[test]
    fn selection_prefers_exact_size_then_smallest_four_by_three() {
        let exact = [
            offer(b"MJPG", &[(320, 240)]),
            offer(b"YUYV", &[(640, 480), (320, 240)]),
        ];
        let choice = select_format(&exact, TARGET).unwrap();
        assert_eq!(
            (choice.format, choice.width, choice.height),
            (PixelFormat::Yuyv, 320, 240)
        );
        // RGB24 beats YUYV when both offer the exact size.
        let both = [offer(b"YUYV", &[(320, 240)]), offer(b"RGB3", &[(320, 240)])];
        assert_eq!(
            select_format(&both, TARGET).unwrap().format,
            PixelFormat::Rgb24
        );
        // No exact match: smallest 4:3 source of at least the target; 16:9 skipped.
        let larger = [offer(
            b"YUYV",
            &[(1280, 720), (640, 360), (640, 480), (352, 288)],
        )];
        let choice = select_format(&larger, TARGET).unwrap();
        assert_eq!((choice.width, choice.height), (640, 480));
        // 160 x 120 processing accepts a 160 x 120 or 320 x 240 source.
        let small = [offer(b"YUYV", &[(320, 240), (160, 120)])];
        let choice = select_format(&small, (160, 120)).unwrap();
        assert_eq!((choice.width, choice.height), (160, 120));
    }

    #[test]
    fn unsupported_offers_report_a_clear_reason() {
        let mjpeg = select_format(&[offer(b"MJPG", &[(640, 480)])], TARGET).unwrap_err();
        assert!(mjpeg.to_string().contains("MJPG"), "{mjpeg}");
        assert!(mjpeg
            .to_string()
            .contains("MJPEG decoding is not supported"));
        let none = select_format(&[], TARGET).unwrap_err();
        assert!(none.to_string().contains("none"));
        // Larger-only is not the problem; smaller-only and oversize-only are.
        for sizes in [
            vec![(160, 120)],
            vec![(1280, 960)],
            vec![(641, 481)],
            vec![(321, 241)],
        ] {
            let error = select_format(&[offer(b"YUYV", &sizes)], TARGET).unwrap_err();
            assert!(error.to_string().contains("no 4:3"), "{sizes:?}");
        }
        // Control bytes in a FourCC are not echoed.
        let odd = select_format(&[offer(&[0x1b, b'[', b'3', 0], &[])], TARGET).unwrap_err();
        assert!(odd.to_string().contains("?[3?"), "{odd}");
    }

    #[test]
    fn negotiation_validates_the_readback_not_the_request() {
        let (clock, log) = fixture();
        let build = |readback: Readback| {
            let mut device = FakeDevice::yuyv(320, 240, &clock, &log);
            device.readback = Ok(readback);
            device
        };
        let good = Readback {
            fourcc: *b"YUYV",
            width: 320,
            height: 240,
            stride: 704,
            size_image: 704 * 240,
        };
        let (choice, layout) = negotiate(&mut build(good), TARGET).unwrap();
        assert_eq!(
            (choice.format, layout.stride, layout.buffer_len),
            (PixelFormat::Yuyv, 704, 168_960)
        );
        for (name, bad) in [
            (
                "fourcc",
                Readback {
                    fourcc: *b"MJPG",
                    ..good
                },
            ),
            ("width", Readback { width: 160, ..good }),
            (
                "height",
                Readback {
                    height: 120,
                    ..good
                },
            ),
        ] {
            let error = negotiate(&mut build(bad), TARGET).unwrap_err();
            assert!(
                matches!(&error, CaptureError::Unsupported(text) if text.contains("replaced")),
                "{name}: {error}"
            );
        }
        for (bad, expected) in [
            (
                Readback {
                    stride: 639,
                    ..good
                },
                FrameError::BadStride,
            ),
            (
                Readback {
                    size_image: 1,
                    ..good
                },
                FrameError::BadBufferSize,
            ),
            (
                Readback {
                    stride: 0x10000,
                    size_image: u32::MAX,
                    ..good
                },
                FrameError::TooLarge,
            ),
            (
                Readback {
                    size_image: 5 * 1024 * 1024,
                    ..good
                },
                FrameError::TooLarge,
            ),
        ] {
            let error = negotiate(&mut build(bad), TARGET).unwrap_err();
            assert_eq!(error, CaptureError::Frame(expected), "{bad:?}");
        }
        // Zero and odd-width YUYV geometry are rejected through the same path.
        let mut zero = FakeDevice::yuyv(320, 240, &clock, &log);
        zero.offers = Ok(vec![offer(b"YUYV", &[(0, 0)])]);
        assert!(negotiate(&mut zero, TARGET).is_err());
        let mut odd = FakeDevice::yuyv(321, 241, &clock, &log);
        odd.offers = Ok(vec![offer(b"YUYV", &[(321, 241)])]);
        assert!(negotiate(&mut odd, (160, 120)).is_err());
        let odd_layout = RawLayout::new(PixelFormat::Yuyv, 321, 240, 642, 642 * 240);
        assert_eq!(odd_layout, Err(FrameError::OddYuyvWidth));
    }

    #[test]
    fn camera_frames_reach_the_converter_at_both_sizes() {
        for target in [(320, 240), (160, 120)] {
            let (clock, log) = fixture();
            let mut device = FakeDevice::yuyv(640, 480, &clock, &log);
            device.script.push_back(frame(10, 1, 81)); // Y=81 everywhere
            let mut camera = session(device, target, &clock);
            wait_until(|| log.waits.load(SeqCst) >= 2);
            clock.set(20);
            assert_eq!(camera.poll(20), CameraPoll::Frame { time_ms: 10 });
            // Fill 81 means Y=81, U=81, V=81: a fixed literal color.
            let expected = crate::frame::yuv_to_rgb(81, 81, 81);
            let rgba = camera.rgba();
            assert_eq!(rgba.len(), target.0 * target.1 * 4);
            assert!(rgba
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [expected[0], expected[1], expected[2], 255]));
            assert!(camera
                .summary()
                .unwrap()
                .contains("YUYV 640x480 stride 1280"));
            assert_eq!(
                camera.poll(21),
                CameraPoll::Idle,
                "one frame is never new twice"
            );
        }
    }

    #[test]
    fn malformed_payloads_and_regressing_metadata_are_never_published() {
        let (clock, log) = fixture();
        let mut device = FakeDevice::yuyv(320, 240, &clock, &log);
        let short = Step::Frame {
            at_ms: 5,
            used: 320 * 2 * 239,
            sequence: 1,
            driver_us: Some(1),
            fill: 1,
        };
        let oversize = Step::Frame {
            at_ms: 6,
            used: 320 * 2 * 240 + 1,
            sequence: 2,
            driver_us: Some(2),
            fill: 1,
        };
        let good = |at_ms, sequence, driver_us| Step::Frame {
            at_ms,
            used: 320 * 2 * 240,
            sequence,
            driver_us: Some(driver_us),
            fill: 100,
        };
        device.script = [
            short,
            oversize,
            good(10, 5, 50),
            good(11, 5, 51), // repeated sequence
            good(12, 6, 50), // equal driver timestamp
            good(13, 7, 49), // regressing driver timestamp
        ]
        .into();
        device.eof_at_end = true;
        let mut camera = session(device, (320, 240), &clock);
        wait_ended(&camera);
        let consumer_view = camera.poll(14);
        // Only the first valid frame was published; EOF then ends the session.
        assert!(
            matches!(&consumer_view, CameraPoll::Fallback(reason) if reason.contains("ended")),
            "{consumer_view:?}"
        );
        let counters = camera.consumer.as_ref().unwrap().latest.counters();
        assert_eq!(counters.published, 1);
    }

    #[test]
    fn startup_and_stall_bounds_are_inclusive_at_their_limits() {
        // Startup: nothing arrives. 2000 ms is allowed, 2001 ms falls back.
        let (clock, log) = fixture();
        let mut camera = session(FakeDevice::yuyv(320, 240, &clock, &log), TARGET, &clock);
        wait_until(|| log.started.load(SeqCst) == 1);
        assert_eq!(camera.poll(2000), CameraPoll::Idle);
        assert!(matches!(camera.poll(2001), CameraPoll::Fallback(r) if r.contains("startup")));
        assert_eq!(camera.poll(5000), CameraPoll::Idle, "falls back once");
        drop(camera);
        assert_eq!(
            (log.stops.load(SeqCst) >= 1, log.dropped.load(SeqCst)),
            (true, 1)
        );

        // Stall: the last complete frame was at 100 ms. 1100 passes, 1101 falls back.
        let (clock, log) = fixture();
        let mut device = FakeDevice::yuyv(320, 240, &clock, &log);
        device.script.push_back(Step::Frame {
            at_ms: 100,
            used: 320 * 2 * 240,
            sequence: 1,
            driver_us: None,
            fill: 16,
        });
        let mut camera = session(device, TARGET, &clock);
        wait_until(|| log.waits.load(SeqCst) >= 2);
        assert_eq!(camera.poll(100), CameraPoll::Frame { time_ms: 100 });
        assert_eq!(camera.poll(1100), CameraPoll::Idle);
        assert!(matches!(camera.poll(1101), CameraPoll::Fallback(r) if r.contains("1 second")));
    }

    #[test]
    fn stale_frames_are_dropped_at_501_ms_and_kept_at_500_ms() {
        for (now, expect_frame) in [(600, true), (601, false)] {
            let (clock, log) = fixture();
            let mut device = FakeDevice::yuyv(320, 240, &clock, &log);
            device.script.push_back(Step::Frame {
                at_ms: 100,
                used: 320 * 2 * 240,
                sequence: 1,
                driver_us: None,
                fill: 128,
            });
            let mut camera = session(device, TARGET, &clock);
            wait_until(|| log.waits.load(SeqCst) >= 2);
            let outcome = camera.poll(now);
            assert_eq!(
                outcome,
                if expect_frame {
                    CameraPoll::Frame { time_ms: 100 }
                } else {
                    CameraPoll::Idle
                }
            );
        }
    }

    #[test]
    fn device_failures_and_eof_become_one_fallback_with_cleanup() {
        for (error, text) in [
            (CaptureError::Eof, "ended"),
            (CaptureError::Disconnected, "disconnected"),
            (CaptureError::Io("EIO".into()), "EIO"),
        ] {
            let (clock, log) = fixture();
            let mut device = FakeDevice::yuyv(320, 240, &clock, &log);
            device.script.push_back(Step::Fail(error));
            let mut camera = session(device, TARGET, &clock);
            wait_ended(&camera);
            assert!(
                matches!(camera.poll(1), CameraPoll::Fallback(r) if r.contains(text)),
                "{text}"
            );
            assert_eq!((log.stops.load(SeqCst), log.dropped.load(SeqCst)), (1, 1));
            wait_until(|| camera.try_reap());
        }
    }

    #[test]
    fn open_failures_and_partial_initialization_release_the_device() {
        // Permission failure before any device exists.
        let (clock, _log) = fixture();
        let opener: Opener = Box::new(|| Err(CaptureError::Permission));
        let mut camera =
            CameraSession::spawn(opener, TARGET, Arc::clone(&clock) as Arc<dyn Clock>).unwrap();
        wait_ended(&camera);
        assert!(matches!(camera.poll(1), CameraPoll::Fallback(r) if r.contains("permission")));

        // Failures after the device opened: offers, readback, start.
        type Break = fn(&mut FakeDevice);
        let breaks: [(&str, Break); 4] = [
            ("offers", |d| {
                d.offers = Err(CaptureError::Io("boom".into()))
            }),
            ("replaced", |d| {
                d.offers = Ok(vec![offer(b"MJPG", &[(640, 480)])])
            }),
            ("mismatch", |d| {
                d.readback = Ok(Readback {
                    width: 160,
                    ..*d.readback.as_ref().unwrap()
                })
            }),
            ("start", |d| {
                d.start = Err(CaptureError::Io("STREAMON".into()))
            }),
        ];
        for (name, apply) in breaks {
            let (clock, log) = fixture();
            let mut device = FakeDevice::yuyv(320, 240, &clock, &log);
            apply(&mut device);
            let mut camera = session(device, TARGET, &clock);
            wait_ended(&camera);
            assert!(matches!(camera.poll(1), CameraPoll::Fallback(_)), "{name}");
            assert_eq!(log.stops.load(SeqCst), 1, "{name}: stop before close");
        }
    }

    #[test]
    fn cancellation_stops_and_closes_a_waiting_or_flooding_worker() {
        // Idle producer: endless timeouts.
        let (clock, log) = fixture();
        let camera = session(FakeDevice::yuyv(320, 240, &clock, &log), TARGET, &clock);
        wait_until(|| log.waits.load(SeqCst) >= 3);
        drop(camera); // cancel + join, as on Escape or window close
        assert_eq!((log.stops.load(SeqCst), log.dropped.load(SeqCst)), (1, 1));

        // Fast producer: frames without end; cancel while it floods.
        let (clock, log) = fixture();
        let mut device = FakeDevice::yuyv(160, 120, &clock, &log);
        device.script = flood(50_000);
        let camera = session(device, (160, 120), &clock);
        wait_until(|| log.waits.load(SeqCst) >= 10);
        drop(camera);
        assert_eq!((log.stops.load(SeqCst), log.dropped.load(SeqCst)), (1, 1));
    }

    fn flood(count: u32) -> std::collections::VecDeque<Step> {
        (1..=count)
            .map(|n| Step::Frame {
                at_ms: u64::from(n),
                used: 160 * 2 * 120,
                sequence: n,
                driver_us: None,
                fill: (n % 255) as u8,
            })
            .collect()
    }

    #[test]
    fn a_fast_producer_without_a_consumer_cannot_grow_storage_or_backlog() {
        let (clock, log) = fixture();
        let mut device = FakeDevice::yuyv(160, 120, &clock, &log);
        device.script = flood(3000);
        device.eof_at_end = true;
        let camera = session(device, (160, 120), &clock);
        wait_ended(&camera);
        let ready = camera.shared.lock().ready.clone().unwrap();
        let counters = ready.latest.counters();
        assert_eq!(counters.published, 3000);
        assert_eq!(
            counters.overwritten, 2999,
            "only the newest frame remains unread"
        );
        assert_eq!(ready.latest.software_bytes(), 3 * 160 * 2 * 120);
        let mut consumer = allocate(ready.layout.buffer_len).unwrap();
        let taken = ready.latest.take(3000, &mut consumer).unwrap();
        assert!(matches!(taken, Taken::Frame(meta) if meta.sequence == 3000));
        assert_eq!(ready.latest.take(3000, &mut consumer), Ok(Taken::Nothing));
    }

    #[test]
    fn real_backend_reports_missing_and_non_video_nodes_without_a_camera() {
        let missing = open_path("/dev/video65535").err().unwrap();
        assert_eq!(missing, CaptureError::Missing);
        // /dev/null opens but is not a V4L2 device; no frame or ioctl succeeds.
        assert!(open_path("/dev/null").is_err());
    }
}
