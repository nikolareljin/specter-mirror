//! Opt-in, bounded measurements for the native kiosk.
//!
//! Everything here observes the render loop; nothing changes pixels, timing
//! policy or buffer ownership. All storage is allocated once in
//! [`Recorder::new`]. Recording an iteration is integer arithmetic on
//! preallocated arrays. The JSON report is formatted once, after the loop and
//! worker cleanup. Reports hold only numbers and fixed labels: no paths,
//! device identifiers, hashes or image data.
//!
//! Units are integer microseconds (`_us`) unless a field says otherwise.

use crate::latest_frame::Counters;
use std::fmt::Write as _;
use std::time::Instant;

pub const SCHEMA: &str = "specter-mirror-native-metrics";
pub const VERSION: u32 = 1;

/// Duration histograms: 400 bins of 250 us cover 0..100 ms.
pub const DURATION_BIN_US: u64 = 250;
pub const DURATION_BINS: usize = 400;
/// Display-interval histograms: 500 bins of 1 ms cover 0..500 ms.
pub const INTERVAL_BIN_US: u64 = 1000;
pub const INTERVAL_BINS: usize = 500;

/// Microsecond monotonic clock, injectable for deterministic tests.
pub trait MicroClock: Send + Sync {
    fn now_us(&self) -> u64;
}

pub struct MonotonicMicros(Instant);

impl MonotonicMicros {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl MicroClock for MonotonicMicros {
    fn now_us(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    Timed,
    Escape,
    WindowClosed,
    Error,
    SelfTest,
}

impl ExitReason {
    fn label(self) -> &'static str {
        match self {
            Self::Timed => "timed",
            Self::Escape => "escape",
            Self::WindowClosed => "window_closed",
            Self::Error => "error",
            Self::SelfTest => "self_test",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Sample,
    Camera,
}

/// What the source did during one display iteration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// No new source data; the previous pixels were shown again.
    Idle,
    /// One new camera frame was converted and processed.
    CameraFrame,
    /// One generated sample update was processed.
    SampleUpdate,
}

/// A bounded percentile summary. `overflow` means the rank fell beyond the
/// histogram range; the value is then the exact maximum, an upper bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Percentile {
    pub value_us: u64,
    pub overflow: bool,
}

/// Fixed-size histogram. Bin `i` holds values in `[i*bin, (i+1)*bin)`;
/// values at or beyond `bin * bins` go to one overflow counter. Count, maximum
/// and overflow are exact. Percentiles use the nearest-rank method and report
/// the inclusive upper bound of the containing bin, capped at the exact
/// maximum, so an in-range value is never understated and is at most one bin too high.
/// Overflow percentiles are upper bounds without a one-bin error guarantee.
pub struct Histogram {
    bin_us: u64,
    bins: Box<[u64]>,
    overflow: u64,
    count: u64,
    max_us: u64,
}

impl Histogram {
    pub fn new(bin_us: u64, bins: usize) -> Self {
        Self {
            bin_us,
            bins: vec![0; bins].into_boxed_slice(),
            overflow: 0,
            count: 0,
            max_us: 0,
        }
    }

    pub fn record(&mut self, value_us: u64) {
        let index = value_us / self.bin_us;
        match usize::try_from(index)
            .ok()
            .and_then(|index| self.bins.get_mut(index))
        {
            Some(bin) => *bin += 1,
            None => self.overflow += 1,
        }
        self.count += 1;
        self.max_us = self.max_us.max(value_us);
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn overflow(&self) -> u64 {
        self.overflow
    }

    pub fn range_us(&self) -> u64 {
        self.bin_us * self.bins.len() as u64
    }

    pub fn max_us(&self) -> Option<u64> {
        (self.count > 0).then_some(self.max_us)
    }

    /// `percent` is 1..=100. `None` for an empty population.
    pub fn percentile(&self, percent: u64) -> Option<Percentile> {
        if self.count == 0 {
            return None;
        }
        let rank = (percent * self.count).div_ceil(100).clamp(1, self.count);
        let mut seen = 0;
        for (index, &bin) in self.bins.iter().enumerate() {
            seen += bin;
            if seen >= rank {
                let upper = (index as u64 + 1) * self.bin_us - 1;
                return Some(Percentile {
                    value_us: upper.min(self.max_us),
                    overflow: false,
                });
            }
        }
        Some(Percentile {
            value_us: self.max_us,
            overflow: true,
        })
    }

    #[cfg(test)]
    fn storage_ptr(&self) -> *const u64 {
        self.bins.as_ptr()
    }
}

struct SourceStats {
    submissions: u64,
    new_items: u64,
    idle_ticks: u64,
    processing: Histogram,
    prepare: Histogram,
    submit: Histogram,
    interval: Histogram,
}

impl SourceStats {
    fn new() -> Self {
        Self {
            submissions: 0,
            new_items: 0,
            idle_ticks: 0,
            processing: Histogram::new(DURATION_BIN_US, DURATION_BINS),
            prepare: Histogram::new(DURATION_BIN_US, DURATION_BINS),
            submit: Histogram::new(DURATION_BIN_US, DURATION_BINS),
            interval: Histogram::new(INTERVAL_BIN_US, INTERVAL_BINS),
        }
    }
}

/// Clock readings for one display iteration (all from the same clock).
#[derive(Clone, Copy, Debug)]
pub struct Iteration {
    /// Source active after the update (a fallback iteration reports Sample).
    pub source: SourceKind,
    pub step: Step,
    /// Before the source update.
    pub step_start_us: u64,
    /// After conversion and temporal processing.
    pub step_end_us: u64,
    /// After the presentation crop/pack.
    pub prepare_end_us: u64,
    /// After the window submission returned.
    pub submit_end_us: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunConfig {
    pub width: usize,
    pub height: usize,
    pub fit: &'static str,
    pub delay_ms: u16,
    pub trails: u8,
    pub reduced_motion: bool,
    pub camera_requested: bool,
    pub run_for_ms: Option<u32>,
}

pub struct Recorder {
    clock: std::sync::Arc<dyn MicroClock>,
    config: RunConfig,
    start_us: u64,
    loop_start_us: Option<u64>,
    camera: SourceStats,
    sample: SourceStats,
    last_submit: Option<(SourceKind, u64)>,
    last_source: Option<SourceKind>,
    first_camera_frame_us: Option<u64>,
    fallback_at_us: Option<u64>,
    capture: Option<Counters>,
    clock_errors: u64,
}

impl Recorder {
    pub fn new(clock: std::sync::Arc<dyn MicroClock>, config: RunConfig) -> Self {
        let start_us = clock.now_us();
        Self {
            clock,
            config,
            start_us,
            loop_start_us: None,
            camera: SourceStats::new(),
            sample: SourceStats::new(),
            last_submit: None,
            last_source: None,
            first_camera_frame_us: None,
            fallback_at_us: None,
            capture: None,
            clock_errors: 0,
        }
    }

    pub fn now_us(&self) -> u64 {
        self.clock.now_us()
    }

    pub fn mark_loop_start(&mut self) {
        self.loop_start_us = Some(self.clock.now_us());
    }

    /// A startup failure has no display loop or worker shutdown population.
    pub fn finish_startup(&mut self, end_us: u64) {
        self.loop_start_us.get_or_insert(end_us);
    }

    pub fn set_capture(&mut self, counters: Option<Counters>) {
        if counters.is_some() {
            self.capture = counters;
        }
    }

    #[cfg(test)]
    pub fn clock_errors(&self) -> u64 {
        self.clock_errors
    }

    fn since_loop(&self, at: u64) -> Option<u64> {
        self.loop_start_us.and_then(|start| at.checked_sub(start))
    }

    /// Records one display iteration. Intervals that would be negative are
    /// discarded and counted in `clock_errors`; no zero is invented.
    pub fn record(&mut self, it: Iteration) {
        let valid = it.step_start_us <= it.step_end_us
            && it.step_end_us <= it.prepare_end_us
            && it.prepare_end_us <= it.submit_end_us;
        if !valid {
            self.clock_errors += 1;
        }
        let interval = match self.last_submit {
            Some((source, before)) if source == it.source => {
                if it.submit_end_us >= before {
                    Some(it.submit_end_us - before)
                } else {
                    self.clock_errors += 1;
                    None
                }
            }
            _ => None,
        };
        if it.source == SourceKind::Sample && self.last_source == Some(SourceKind::Camera) {
            self.fallback_at_us = self.since_loop(it.submit_end_us);
        }
        if it.step == Step::CameraFrame && self.first_camera_frame_us.is_none() {
            self.first_camera_frame_us = self.since_loop(it.step_end_us);
        }
        self.last_submit = Some((it.source, it.submit_end_us));
        self.last_source = Some(it.source);
        let stats = match it.source {
            SourceKind::Camera => &mut self.camera,
            SourceKind::Sample => &mut self.sample,
        };
        stats.submissions += 1;
        match it.step {
            Step::Idle => stats.idle_ticks += 1,
            Step::CameraFrame | Step::SampleUpdate => stats.new_items += 1,
        }
        if let Some(interval) = interval {
            stats.interval.record(interval);
        }
        if valid {
            if it.step != Step::Idle {
                stats.processing.record(it.step_end_us - it.step_start_us);
            }
            stats.prepare.record(it.prepare_end_us - it.step_end_us);
            stats.submit.record(it.submit_end_us - it.prepare_end_us);
        }
    }

    /// Formats the single report. Call after worker cleanup.
    pub fn report(&self, reason: ExitReason, loop_end_us: u64, end_us: u64) -> String {
        let mut out = String::with_capacity(4096);
        let elapsed = end_us.checked_sub(self.start_us);
        let startup = self
            .loop_start_us
            .and_then(|start| start.checked_sub(self.start_us));
        let loop_us = self
            .loop_start_us
            .and_then(|start| loop_end_us.checked_sub(start));
        let shutdown = end_us.checked_sub(loop_end_us);
        let bad_run = [elapsed, startup, loop_us, shutdown]
            .iter()
            .filter(|value| value.is_none())
            .count() as u64;
        let c = &self.config;
        let final_source = match self.last_source {
            Some(SourceKind::Camera) => "camera",
            _ => "sample",
        };
        let fallback = c.camera_requested && final_source != "camera";
        let _ = write!(
            out,
            "{{\"schema\":\"{SCHEMA}\",\"version\":{VERSION},\"run\":{{\"exit_reason\":\"{}\",\"requested_run_ms\":",
            reason.label()
        );
        opt(&mut out, c.run_for_ms.map(u64::from));
        out.push_str(",\"elapsed_us\":");
        opt(&mut out, elapsed);
        out.push_str(",\"startup_us\":");
        opt(&mut out, startup);
        out.push_str(",\"loop_us\":");
        opt(&mut out, loop_us);
        out.push_str(",\"shutdown_us\":");
        opt(&mut out, shutdown);
        let _ = write!(
            out,
            ",\"clock_errors\":{}}},\"config\":{{\"width\":{},\"height\":{},\"fit\":\"{}\",\"delay_ms\":{},\"trails\":{},\"reduced_motion\":{},\"target_fps\":30}},",
            self.clock_errors + bad_run,
            c.width,
            c.height,
            c.fit,
            c.delay_ms,
            c.trails,
            c.reduced_motion
        );
        let _ = write!(
            out,
            "\"source\":{{\"requested\":\"{}\",\"final\":\"{final_source}\",\"fallback_occurred\":{fallback},\"fallback_at_us\":",
            if c.camera_requested { "camera" } else { "sample" }
        );
        opt(&mut out, self.fallback_at_us.filter(|_| fallback));
        out.push_str(",\"first_camera_frame_us\":");
        opt(&mut out, self.first_camera_frame_us);
        let _ = write!(
            out,
            "}},\"display_submissions\":{},",
            self.camera.submissions + self.sample.submissions
        );
        out.push_str("\"camera\":");
        source_json(&mut out, &self.camera, "frames");
        out.push_str(",\"capture\":");
        match self.capture {
            None => out.push_str("null"),
            Some(counters) => {
                let _ = write!(
                    out,
                    "{{\"published\":{},\"overwritten_unread\":{},\"stale_dropped\":{},\"driver_drops\":null}}",
                    counters.published, counters.overwritten, counters.stale
                );
            }
        }
        out.push_str(",\"sample\":");
        source_json(&mut out, &self.sample, "updates");
        out.push_str("}\n");
        out
    }
}

fn opt(out: &mut String, value: Option<u64>) {
    match value {
        Some(value) => {
            let _ = write!(out, "{value}");
        }
        None => out.push_str("null"),
    }
}

fn percentile_json(out: &mut String, histogram: &Histogram, percent: u64) {
    match histogram.percentile(percent) {
        Some(p) => {
            let _ = write!(
                out,
                "\"p{percent}_us\":{},\"p{percent}_overflow\":{}",
                p.value_us, p.overflow
            );
        }
        None => {
            let _ = write!(out, "\"p{percent}_us\":null,\"p{percent}_overflow\":null");
        }
    }
}

fn summary_json(out: &mut String, histogram: &Histogram) {
    let _ = write!(out, "{{\"count\":{},", histogram.count());
    percentile_json(out, histogram, 50);
    out.push(',');
    percentile_json(out, histogram, 95);
    out.push_str(",\"max_us\":");
    opt(out, histogram.max_us());
    let _ = write!(
        out,
        ",\"bin_us\":{},\"range_us\":{},\"overflow_count\":{}}}",
        histogram.bin_us,
        histogram.range_us(),
        histogram.overflow()
    );
}

fn source_json(out: &mut String, stats: &SourceStats, new_label: &str) {
    let _ = write!(
        out,
        "{{\"submissions\":{},\"{new_label}\":{},\"idle_ticks\":{},\"processing_us\":",
        stats.submissions, stats.new_items, stats.idle_ticks
    );
    summary_json(out, &stats.processing);
    out.push_str(",\"prepare_us\":");
    summary_json(out, &stats.prepare);
    out.push_str(",\"submit_us\":");
    summary_json(out, &stats.submit);
    out.push_str(",\"display_interval_us\":");
    summary_json(out, &stats.interval);
    out.push('}');
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    struct Manual(AtomicU64);
    impl MicroClock for Manual {
        fn now_us(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }

    fn config(camera: bool) -> RunConfig {
        RunConfig {
            width: 320,
            height: 240,
            fit: "cover",
            delay_ms: 250,
            trails: 115,
            reduced_motion: false,
            camera_requested: camera,
            run_for_ms: Some(1000),
        }
    }

    fn recorder(camera: bool) -> Recorder {
        let mut recorder = Recorder::new(Arc::new(Manual(AtomicU64::new(100))), config(camera));
        recorder.loop_start_us = Some(1100);
        recorder
    }

    fn it(source: SourceKind, step: Step, at: [u64; 4]) -> Iteration {
        Iteration {
            source,
            step,
            step_start_us: at[0],
            step_end_us: at[1],
            prepare_end_us: at[2],
            submit_end_us: at[3],
        }
    }

    fn hist(values: &[u64]) -> Histogram {
        let mut h = Histogram::new(DURATION_BIN_US, DURATION_BINS);
        values.iter().for_each(|&v| h.record(v));
        h
    }

    #[test]
    fn empty_population_is_unavailable_not_zero() {
        let h = hist(&[]);
        assert_eq!((h.count(), h.max_us(), h.percentile(50)), (0, None, None));
        let text = recorder(false).report(ExitReason::Timed, 2000, 2500);
        assert!(text.contains("\"p50_us\":null,\"p50_overflow\":null"));
        assert!(text.contains("\"max_us\":null"));
        assert!(text.contains("\"capture\":null"));
    }

    #[test]
    fn startup_failure_reports_elapsed_startup_without_a_display_population() {
        let mut r = Recorder::new(Arc::new(Manual(AtomicU64::new(100))), config(false));
        r.finish_startup(600);
        let text = r.report(ExitReason::Error, 600, 600);
        assert!(text.contains("\"exit_reason\":\"error\""));
        assert!(
            text.contains("\"elapsed_us\":500,\"startup_us\":500,\"loop_us\":0,\"shutdown_us\":0")
        );
        assert!(text.contains("\"display_submissions\":0"));
    }

    #[test]
    fn single_sample_reports_the_exact_value_for_every_percentile() {
        let h = hist(&[1234]);
        for p in [50, 95] {
            assert_eq!(
                h.percentile(p),
                Some(Percentile {
                    value_us: 1234,
                    overflow: false
                })
            );
        }
        assert_eq!(h.max_us(), Some(1234));
    }

    #[test]
    fn fixed_samples_match_independent_nearest_rank_within_one_bin() {
        // Twenty samples, 1000..=20000 us in 1000 us steps. Nearest rank:
        // p50 = rank 10 = 10000 us, p95 = rank 19 = 19000 us, max 20000 us.
        let values: Vec<u64> = (1..=20).map(|n| n * 1000).collect();
        let h = hist(&values);
        let (p50, p95) = (h.percentile(50).unwrap(), h.percentile(95).unwrap());
        assert!((10_000..10_000 + DURATION_BIN_US).contains(&p50.value_us));
        assert!((19_000..19_000 + DURATION_BIN_US).contains(&p95.value_us));
        // Bin of 10000 is [10000, 10250): inclusive upper bound 10249.
        assert_eq!((p50.value_us, p95.value_us), (10_249, 19_249));
        assert_eq!(h.max_us(), Some(20_000));
        assert!(!p50.overflow && !p95.overflow);
        // Unordered input gives the same answer.
        let mut shuffled = values.clone();
        shuffled.reverse();
        assert_eq!(hist(&shuffled).percentile(95), h.percentile(95));
        // Percentiles never exceed the exact maximum.
        assert_eq!(hist(&[10, 10, 10]).percentile(95).unwrap().value_us, 10);
    }

    #[test]
    fn overflow_is_counted_reported_and_clips_to_the_exact_maximum() {
        let range = DURATION_BIN_US * DURATION_BINS as u64;
        let h = hist(&[100, range - 1, range, range * 5, u64::MAX]);
        assert_eq!((h.count(), h.overflow()), (5, 3));
        assert_eq!(h.max_us(), Some(u64::MAX));
        // Rank 3 of 5 lies in the overflow population.
        assert_eq!(
            h.percentile(50),
            Some(Percentile {
                value_us: u64::MAX,
                overflow: true
            })
        );
        let low = hist(&[100, 200, 300, range]);
        assert!(!low.percentile(50).unwrap().overflow);
        assert!(low.percentile(95).unwrap().overflow);
    }

    #[test]
    fn long_runs_reuse_fixed_storage() {
        let mut h = hist(&[]);
        let pointer = h.storage_ptr();
        for n in 0..2_000_000u64 {
            h.record(n % 150_000);
        }
        assert_eq!(h.storage_ptr(), pointer);
        assert_eq!(h.count(), 2_000_000);
        let mut recorder = recorder(false);
        let pointer = recorder.sample.processing.storage_ptr();
        for n in 0..100_000u64 {
            let base = 2000 + n * 100;
            recorder.record(it(
                SourceKind::Sample,
                Step::SampleUpdate,
                [base, base + 10, base + 20, base + 30],
            ));
        }
        assert_eq!(recorder.sample.processing.storage_ptr(), pointer);
        assert_eq!(recorder.sample.new_items, 100_000);
    }

    #[test]
    fn idle_ticks_are_submissions_but_never_new_frames_or_processing() {
        let mut r = recorder(true);
        r.record(it(SourceKind::Camera, Step::Idle, [2000, 2001, 2010, 2040]));
        r.record(it(
            SourceKind::Camera,
            Step::CameraFrame,
            [2040, 4040, 4100, 4150],
        ));
        r.record(it(SourceKind::Camera, Step::Idle, [4150, 4151, 4160, 4190]));
        assert_eq!(
            (
                r.camera.submissions,
                r.camera.new_items,
                r.camera.idle_ticks
            ),
            (3, 1, 2)
        );
        assert_eq!(r.camera.processing.count(), 1);
        assert_eq!(r.camera.processing.max_us(), Some(2000));
        // Presentation is measured on idle ticks too, separately.
        assert_eq!(r.camera.prepare.count(), 3);
        assert_eq!(r.camera.submit.count(), 3);
        // Two intervals: 4150-2040 and 4190-4150.
        assert_eq!(r.camera.interval.count(), 2);
        assert_eq!(r.camera.interval.max_us(), Some(2110));
        assert_eq!(r.sample.submissions, 0);
    }

    #[test]
    fn sources_stay_separate_across_fallback() {
        let mut r = recorder(true);
        r.record(it(
            SourceKind::Camera,
            Step::CameraFrame,
            [2000, 2500, 2600, 2700],
        ));
        r.record(it(
            SourceKind::Sample,
            Step::SampleUpdate,
            [5000, 5300, 5400, 5500],
        ));
        r.record(it(
            SourceKind::Sample,
            Step::SampleUpdate,
            [5500, 5800, 5900, 6000],
        ));
        assert_eq!(r.camera.processing.count(), 1);
        assert_eq!(r.sample.processing.count(), 2);
        assert_eq!(r.camera.interval.count(), 0);
        // The interval spanning the switch is not attributed to either source.
        assert_eq!(r.sample.interval.count(), 1);
        assert_eq!(r.sample.interval.max_us(), Some(500));
        assert_eq!(r.first_camera_frame_us, Some(1400));
        assert_eq!(r.fallback_at_us, Some(4400));
        let text = r.report(ExitReason::Timed, 6000, 7000);
        assert!(text.contains("\"requested\":\"camera\",\"final\":\"sample\",\"fallback_occurred\":true,\"fallback_at_us\":4400"));
    }

    #[test]
    fn camera_never_started_is_fallback_evidence_with_no_frames() {
        let r = recorder(true);
        let text = r.report(ExitReason::Timed, 2000, 2100);
        assert!(text.contains(
            "\"fallback_occurred\":true,\"fallback_at_us\":null,\"first_camera_frame_us\":null"
        ));
        assert!(text.contains("\"frames\":0"));
    }

    #[test]
    fn invalid_clock_values_are_counted_and_not_recorded() {
        let mut r = recorder(false);
        r.record(it(
            SourceKind::Sample,
            Step::SampleUpdate,
            [2000, 1900, 2100, 2200],
        ));
        r.record(it(
            SourceKind::Sample,
            Step::SampleUpdate,
            [2300, 2400, 2500, 2100],
        ));
        assert_eq!(r.sample.submissions, 2);
        assert_eq!(r.sample.processing.count(), 0);
        assert_eq!(r.sample.prepare.count(), 0);
        assert_eq!(r.sample.interval.count(), 0);
        assert_eq!(r.clock_errors(), 3);
        // A run whose end precedes its start reports null durations.
        let text = r.report(ExitReason::Error, 1000, 50);
        assert!(text.contains("\"elapsed_us\":null"));
        assert!(text.contains("\"shutdown_us\":null"));
    }

    #[test]
    fn report_has_exact_run_fields_capture_counters_and_no_invented_values() {
        let mut r = recorder(true);
        r.set_capture(Some(Counters {
            published: 9,
            overwritten: 3,
            stale: 1,
        }));
        r.set_capture(None); // an unavailable later snapshot keeps the earlier one
        r.record(it(
            SourceKind::Camera,
            Step::CameraFrame,
            [2000, 2400, 2500, 2600],
        ));
        let text = r.report(ExitReason::Escape, 3100, 3600);
        assert!(text.starts_with(
            "{\"schema\":\"specter-mirror-native-metrics\",\"version\":1,\"run\":{\"exit_reason\":\"escape\",\"requested_run_ms\":1000,\"elapsed_us\":3500,\"startup_us\":1000,\"loop_us\":2000,\"shutdown_us\":500,\"clock_errors\":0}"
        ));
        assert!(text.contains("\"capture\":{\"published\":9,\"overwritten_unread\":3,\"stale_dropped\":1,\"driver_drops\":null}"));
        assert!(text.contains("\"processing_us\":{\"count\":1,\"p50_us\":400,\"p50_overflow\":false,\"p95_us\":400,\"p95_overflow\":false,\"max_us\":400,\"bin_us\":250,\"range_us\":100000,\"overflow_count\":0}"));
        assert!(text.contains("\"display_interval_us\":{\"count\":0,\"p50_us\":null"));
        assert!(text.ends_with("}\n") && text.matches('\n').count() == 1);
        assert!(!text.contains("NaN") && !text.contains("inf"));
    }
}
