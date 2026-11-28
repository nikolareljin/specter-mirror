use minifb::{InputCallback, Key, Window, WindowOptions};
use spook_core::{TemporalError, TemporalOptions};
use std::cell::Cell;
use std::error::Error;
use std::rc::Rc;
use std::sync::Arc;

mod camera;
mod frame;
mod latest_frame;
mod metrics;
mod presentation;
mod sample;
use camera::{CameraPoll, CameraSession, Clock, MonotonicClock};
use latest_frame::Counters;
use metrics::{
    ExitReason, Iteration, MicroClock, MonotonicMicros, Recorder, RunConfig, SourceKind, Step,
};
use presentation::{FitMode, Presentation};
use sample::Sample;

const HELP: &str = "Usage: spook-kiosk [options]\n\
Display an animated generated sample, or an opt-in V4L2 camera, through\n\
CPU-only X11. Escape exits.\n\
  --fit cover     Fill the window, cropping the center (default).\n\
  --fit contain   Preserve the complete image with letterboxing.\n\
  --fit stretch   Fill the window without preserving aspect ratio.\n\
  --camera PATH   Use /dev/videoN (absolute path, decimal N) instead of the\n\
                  sample. RGB24 or YUYV only; falls back to the sample.\n\
  --self-test     Render one frame, then exit (sample only).\n\
  --metrics       Print one JSON measurement report to stdout at exit.\n\
  --run-for-ms N  Exit normally after N ms, 100..600000.\n\
  --delay-ms N    Reflection delay, 0..600 ms (default 250).\n\
  --trails N      Past-frame blend weight, 0..255 (default 115).\n\
  --resolution S Processing size: 320x240 (default) or 160x120.\n\
  --reduced-motion  Freeze the sample and disable delay/trails; camera\n\
                  frames stay live without delay/trails.\n\
  -h, --help      Print help without opening a window.\n\
Examples:\n\
  spook-kiosk\n\
  spook-kiosk --fit contain\n\
  spook-kiosk --resolution 160x120 --delay-ms 400 --trails 128\n\
  spook-kiosk --reduced-motion\n\
  spook-kiosk --camera /dev/video0 --resolution 160x120\n\
  spook-kiosk --self-test --fit cover\n\
  spook-kiosk --metrics --run-for-ms 30000\n\
The appliance session requests fullscreen. Camera mode is opt-in, needs read/write\n\
access to the device and has no hardware verification yet. --metrics and\n\
--run-for-ms cannot be combined with --self-test.";

#[derive(Debug, PartialEq, Eq)]
struct Options {
    proof: bool,
    help: bool,
    fit: FitMode,
    temporal: TemporalOptions,
    low_resolution: bool,
    reduced_motion: bool,
    /// Index N of an explicit `/dev/videoN`; `None` selects the sample.
    camera: Option<u16>,
    metrics: bool,
    /// Timed normal exit after this many milliseconds (100..=600000).
    run_for_ms: Option<u32>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            proof: false,
            help: false,
            fit: FitMode::default(),
            temporal: TemporalOptions {
                delay_ms: 250,
                trail_strength: 115,
            },
            low_resolution: false,
            reduced_motion: false,
            camera: None,
            metrics: false,
            run_for_ms: None,
        }
    }
}

fn parse_options(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Options, &'static str> {
    let mut options = Options::default();
    let mut fit_seen = false;
    let mut delay_seen = false;
    let mut trails_seen = false;
    let mut resolution_seen = false;
    let mut arguments = arguments.into_iter().peekable();
    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--self-test") if !options.proof && !options.help => options.proof = true,
            Some("--fit") if !fit_seen && !options.help => {
                options.fit = match arguments
                    .next()
                    .as_deref()
                    .and_then(std::ffi::OsStr::to_str)
                {
                    Some("cover") => FitMode::Cover,
                    Some("contain") => FitMode::Contain,
                    Some("stretch") => FitMode::Stretch,
                    _ => return Err(HELP),
                };
                fit_seen = true;
            }
            Some("--delay-ms") if !delay_seen => {
                options.temporal.delay_ms = parse_number(arguments.next(), 600)?;
                delay_seen = true;
            }
            Some("--trails") if !trails_seen => {
                options.temporal.trail_strength = parse_number(arguments.next(), 255)? as u8;
                trails_seen = true;
            }
            Some("--resolution") if !resolution_seen => {
                options.low_resolution = match arguments
                    .next()
                    .as_deref()
                    .and_then(std::ffi::OsStr::to_str)
                {
                    Some("160x120") => true,
                    Some("320x240") => false,
                    _ => return Err(HELP),
                };
                resolution_seen = true;
            }
            Some("--camera") if options.camera.is_none() && !options.help => {
                options.camera = Some(parse_camera(arguments.next())?);
            }
            Some("--metrics") if !options.metrics && !options.help => options.metrics = true,
            Some("--run-for-ms") if options.run_for_ms.is_none() && !options.help => {
                options.run_for_ms = Some(parse_run_for(arguments.next())?);
            }
            Some("--reduced-motion") if !options.reduced_motion => options.reduced_motion = true,
            Some("--help" | "-h")
                if options == Options::default()
                    && !fit_seen
                    && !delay_seen
                    && !trails_seen
                    && !resolution_seen
                    && arguments.peek().is_none() =>
            {
                options.help = true;
            }
            _ => return Err(HELP),
        }
    }
    if options.proof
        && (options.camera.is_some() || options.metrics || options.run_for_ms.is_some())
    {
        return Err(HELP);
    }
    Ok(options)
}

/// Accepts only `/dev/video` followed by a canonical decimal index.
fn parse_camera(value: Option<std::ffi::OsString>) -> Result<u16, &'static str> {
    let text = value
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or(HELP)?;
    let digits = text.strip_prefix("/dev/video").ok_or(HELP)?;
    let canonical = !digits.is_empty()
        && digits.len() <= 5
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits.len() == 1 || !digits.starts_with('0'));
    if !canonical {
        return Err(HELP);
    }
    digits.parse::<u16>().map_err(|_| HELP)
}

/// Accepts a canonical decimal number of milliseconds in 100..=600000.
fn parse_run_for(value: Option<std::ffi::OsString>) -> Result<u32, &'static str> {
    let text = value
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or(HELP)?;
    let canonical = !text.is_empty()
        && text.len() <= 6
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && !text.starts_with('0');
    if !canonical {
        return Err(HELP);
    }
    let number = text.parse::<u32>().map_err(|_| HELP)?;
    if !(100..=600_000).contains(&number) {
        return Err(HELP);
    }
    Ok(number)
}

fn parse_number(value: Option<std::ffi::OsString>, max: u16) -> Result<u16, &'static str> {
    let text = value
        .as_deref()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or(HELP)?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(HELP);
    }
    let number = text.parse::<u16>().map_err(|_| HELP)?;
    if number > max {
        return Err(HELP);
    }
    Ok(number)
}

struct ExitKeys(Rc<Cell<bool>>);

impl InputCallback for ExitKeys {
    fn add_char(&mut self, _character: u32) {}

    fn set_key_state(&mut self, key: Key, down: bool) {
        if key == Key::Escape && down {
            self.0.set(true);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Sample,
    Camera,
}

/// Owns the active source and the shared temporal output. The camera is
/// replaced by the sample at most once; the retired session is joined as soon
/// as its worker finishes, and at the latest on drop.
struct Pipeline {
    sample: Sample,
    camera: Option<CameraSession>,
    retiring: Option<CameraSession>,
    source: Source,
    announced: bool,
    camera_frames: u64,
    /// What the most recent `update` did; read by measurement only.
    last_step: Step,
    /// Latest numeric capture snapshot, kept after the session is released.
    capture: Option<Counters>,
}

impl Pipeline {
    fn new(sample: Sample, camera: Option<CameraSession>) -> Self {
        Self {
            sample,
            source: if camera.is_some() {
                Source::Camera
            } else {
                Source::Sample
            },
            camera,
            retiring: None,
            announced: false,
            camera_frames: 0,
            last_step: Step::Idle,
            capture: None,
        }
    }

    fn source_kind(&self) -> SourceKind {
        match self.source {
            Source::Sample => SourceKind::Sample,
            Source::Camera => SourceKind::Camera,
        }
    }

    fn pixels(&self) -> &[u32] {
        self.sample.pixels()
    }

    /// Advances the active source to `now_ms`. Returns a concise diagnostic
    /// when the source state changes. Camera trouble never fails the update:
    /// it switches to the sample.
    fn update(
        &mut self,
        now_ms: u64,
        options: TemporalOptions,
        reduced_motion: bool,
    ) -> Result<Option<String>, TemporalError> {
        self.last_step = Step::Idle;
        if self.retiring.as_mut().is_some_and(CameraSession::try_reap) {
            if let Some(done) = self.retiring.take() {
                self.capture = done.finish().or(self.capture);
            }
        }
        let mut notice = None;
        if self.source == Source::Camera {
            let poll = self.camera.as_mut().map_or_else(
                || CameraPoll::Fallback("camera session missing".into()),
                |c| c.poll(now_ms),
            );
            match poll {
                CameraPoll::Idle => return Ok(None),
                CameraPoll::Frame { time_ms } => {
                    let camera = self.camera.as_ref().expect("a frame implies a session");
                    // Camera motion is live; reduced motion only drops delay and trails.
                    let effective = if reduced_motion {
                        TemporalOptions {
                            delay_ms: 0,
                            trail_strength: 0,
                        }
                    } else {
                        options
                    };
                    match self
                        .sample
                        .process_external(camera.rgba(), time_ms, effective)
                    {
                        Ok(()) => {
                            self.camera_frames += 1;
                            self.last_step = Step::CameraFrame;
                            if !self.announced {
                                self.announced = true;
                                notice = camera.summary().map(|text| format!("camera: {text}"));
                            }
                            return Ok(notice);
                        }
                        Err(error) => {
                            notice = Some(self.fall_back(&format!("frames rejected ({error})")))
                        }
                    }
                }
                CameraPoll::Fallback(reason) => notice = Some(self.fall_back(&reason)),
            }
        }
        self.sample.update(now_ms, options, reduced_motion)?;
        self.last_step = Step::SampleUpdate;
        Ok(notice)
    }

    /// Switches to the sample for good and clears every camera pixel from
    /// the shared history before the sample writes into it.
    fn fall_back(&mut self, reason: &str) -> String {
        if let Some(camera) = self.camera.take() {
            camera.cancel();
            // The worker may still publish while it winds down; the final
            // counters replace this snapshot once it is joined.
            self.capture = camera.capture_counters().or(self.capture);
            self.retiring = Some(camera);
        }
        self.source = Source::Sample;
        self.sample.reset();
        format!("camera: {reason}; using the generated sample")
    }

    /// Cancels and joins every worker, closing the device.
    fn shutdown(&mut self) {
        for session in [self.camera.take(), self.retiring.take()]
            .into_iter()
            .flatten()
        {
            self.capture = session.finish().or(self.capture);
        }
    }
}

/// A source notice and, when measuring, the clock readings around the update.
type MeasuredUpdate = (Option<String>, Option<(u64, u64)>);

/// Runs one source update. When measuring, returns the clock readings
/// around it; reading a clock never touches the pipeline.
fn update_measured(
    pipeline: &mut Pipeline,
    recorder: Option<&Recorder>,
    now_ms: u64,
    options: TemporalOptions,
    reduced_motion: bool,
) -> Result<MeasuredUpdate, TemporalError> {
    let start = recorder.map(Recorder::now_us);
    let notice = pipeline.update(now_ms, options, reduced_motion)?;
    let span = start.zip(recorder.map(Recorder::now_us));
    Ok((notice, span))
}

fn fit_label(fit: FitMode) -> &'static str {
    match fit {
        FitMode::Cover => "cover",
        FitMode::Contain => "contain",
        FitMode::Stretch => "stretch",
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let options = parse_options(std::env::args_os().skip(1))?;
    if options.help {
        println!("{HELP}");
        return Ok(());
    }
    let (width, height) = if options.low_resolution {
        (160, 120)
    } else {
        (320, 240)
    };
    let micros: Option<Arc<dyn MicroClock>> = (options.metrics || options.run_for_ms.is_some())
        .then(|| Arc::new(MonotonicMicros::new()) as Arc<dyn MicroClock>);
    let mut recorder = options.metrics.then(|| {
        Recorder::new(
            Arc::clone(micros.as_ref().expect("metrics needs a clock")),
            RunConfig {
                width,
                height,
                fit: fit_label(options.fit),
                delay_ms: options.temporal.delay_ms,
                trails: options.temporal.trail_strength,
                reduced_motion: options.reduced_motion,
                camera_requested: options.camera.is_some(),
                run_for_ms: options.run_for_ms,
            },
        )
    });
    let mut loop_end = None;
    let result = (|| -> Result<ExitReason, Box<dyn Error>> {
        let sample = Sample::new(width, height)?;
        let mut window = Window::new(
            "SpecterMirror - synthetic frame proof",
            width,
            height,
            WindowOptions {
                resize: true,
                scale_mode: if options.fit == FitMode::Contain {
                    minifb::ScaleMode::AspectRatioStretch
                } else {
                    minifb::ScaleMode::Stretch
                },
                ..WindowOptions::default()
            },
        )?;
        let exit_requested = Rc::new(Cell::new(false));
        window.set_input_callback(Box::new(ExitKeys(Rc::clone(&exit_requested))));
        window.set_target_fps(30);
        let mut presentation = Presentation::new(width, height)?;
        let clock = Arc::new(MonotonicClock::new());
        let camera = options.camera.and_then(|index| {
            match CameraSession::spawn(
                camera::open_index(index),
                (width, height),
                Arc::clone(&clock) as Arc<dyn Clock>,
            ) {
                Ok(session) => Some(session),
                Err(error) => {
                    eprintln!(
                        "camera: cannot start capture worker ({error}); using the generated sample"
                    );
                    None
                }
            }
        });
        let mut pipeline = Pipeline::new(sample, camera);
        if let Some(recorder) = recorder.as_mut() {
            recorder.mark_loop_start();
        }
        // Only a timed run reads a clock for its deadline. It starts after the
        // loop start mark, so a reported loop is never shorter than requested.
        let deadline_us = options.run_for_ms.map(|ms| {
            micros
                .as_ref()
                .expect("timed run needs a clock")
                .now_us()
                .saturating_add(u64::from(ms) * 1000)
        });
        let result = (|| -> Result<ExitReason, Box<dyn Error>> {
            loop {
                if !window.is_open() {
                    return Ok(ExitReason::WindowClosed);
                }
                if exit_requested.get() {
                    return Ok(ExitReason::Escape);
                }
                if deadline_us.is_some_and(|deadline| {
                    micros.as_ref().expect("timed run needs a clock").now_us() >= deadline
                }) {
                    return Ok(ExitReason::Timed);
                }
                let (display_width, display_height) = window.get_size();
                if display_width == 0 || display_height == 0 {
                    window.update();
                    continue;
                }
                let (notice, span) = update_measured(
                    &mut pipeline,
                    recorder.as_ref(),
                    clock.now_ms(),
                    options.temporal,
                    options.reduced_motion,
                )?;
                presentation.update(
                    pipeline.pixels(),
                    display_width,
                    display_height,
                    options.fit,
                )?;
                let prepare_end = recorder.as_ref().map(Recorder::now_us);
                let crop = presentation.crop();
                window.update_with_buffer(presentation.pixels(), crop.width, crop.height)?;
                if let (Some(recorder), Some((step_start, step_end)), Some(prepare_end)) =
                    (recorder.as_mut(), span, prepare_end)
                {
                    let submit_end = recorder.now_us();
                    recorder.record(Iteration {
                        source: pipeline.source_kind(),
                        step: pipeline.last_step,
                        step_start_us: step_start,
                        step_end_us: step_end,
                        prepare_end_us: prepare_end,
                        submit_end_us: submit_end,
                    });
                }
                if let Some(notice) = notice {
                    eprintln!("{notice}");
                }
                if options.proof {
                    println!("Rendered a synthetic frame through the X11 software window.");
                    return Ok(ExitReason::SelfTest);
                }
            }
        })();
        loop_end = recorder.as_ref().map(Recorder::now_us);
        pipeline.shutdown();
        if let Some(recorder) = recorder.as_mut() {
            recorder.set_capture(pipeline.capture);
        }
        result
    })();
    let reason = result.as_ref().map_or(ExitReason::Error, |reason| *reason);
    let report = recorder.as_mut().map(|recorder| {
        let end = recorder.now_us();
        let loop_end = loop_end.unwrap_or(end);
        recorder.finish_startup(loop_end);
        recorder.report(reason, loop_end, end)
    });
    if let Some(report) = report {
        use std::io::Write as _;
        let mut stdout = std::io::stdout().lock();
        if let Err(error) = stdout
            .write_all(report.as_bytes())
            .and_then(|()| stdout.flush())
        {
            result?;
            return Err(error.into());
        }
    }
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn cover_is_default_and_each_fit_is_accepted() {
        assert_eq!(parse_options(args(&[])).unwrap(), Options::default());
        for (name, fit) in [
            ("cover", FitMode::Cover),
            ("contain", FitMode::Contain),
            ("stretch", FitMode::Stretch),
        ] {
            let options = parse_options(args(&["--self-test", "--fit", name])).unwrap();
            assert_eq!(
                options,
                Options {
                    proof: true,
                    help: false,
                    fit,
                    ..Options::default()
                }
            );
            assert_eq!(
                parse_options(args(&["--fit", name, "--self-test"])).unwrap(),
                options
            );
        }
    }

    #[test]
    fn temporal_and_resolution_arguments_are_bounded() {
        let options = parse_options(args(&[
            "--trails",
            "255",
            "--delay-ms",
            "600",
            "--resolution",
            "160x120",
            "--reduced-motion",
        ]))
        .unwrap();
        assert_eq!(
            options.temporal,
            TemporalOptions {
                delay_ms: 600,
                trail_strength: 255
            }
        );
        assert!(options.low_resolution && options.reduced_motion);
        assert!(
            !parse_options(args(&["--resolution", "320x240"]))
                .unwrap()
                .low_resolution
        );
        for values in [
            vec!["--delay-ms"],
            vec!["--delay-ms", "601"],
            vec!["--delay-ms", "-1"],
            vec!["--delay-ms", "+1"],
            vec!["--delay-ms", "1.0"],
            vec!["--delay-ms", "65536"],
            vec!["--trails", "256"],
            vec!["--trails", "NaN"],
            vec!["--resolution", "640x480"],
            vec!["--delay-ms", "0", "--delay-ms", "1"],
            vec!["--trails", "0", "--trails", "0"],
            vec!["--resolution", "320x240", "--resolution", "160x120"],
            vec!["--reduced-motion", "--reduced-motion"],
            vec!["--trails", "115", "--help"],
        ] {
            assert!(parse_options(args(&values)).is_err(), "{values:?}");
        }
    }

    #[test]
    fn help_is_explicit_and_bad_arguments_are_rejected() {
        for name in ["-h", "--help"] {
            assert!(parse_options(args(&[name])).unwrap().help);
        }
        for values in [
            vec!["--fit"],
            vec!["--fit", "bad"],
            vec!["--unknown"],
            vec!["--self-test", "--self-test"],
            vec!["--help", "--self-test"],
            vec!["--self-test", "--help"],
            vec!["--fit", "cover", "--fit", "stretch"],
        ] {
            assert!(parse_options(args(&values)).is_err());
        }
    }

    #[test]
    fn escape_press_is_retained_after_release() {
        let requested = Rc::new(Cell::new(false));
        let mut keys = ExitKeys(Rc::clone(&requested));
        keys.set_key_state(Key::A, true);
        keys.set_key_state(Key::A, false);
        keys.add_char(u32::from(b'a'));
        assert!(!requested.get());
        keys.set_key_state(Key::Escape, true);
        keys.set_key_state(Key::Escape, false);
        assert!(requested.get());
    }

    #[test]
    fn camera_argument_is_explicit_canonical_and_exclusive() {
        let ok = parse_options(args(&["--camera", "/dev/video0"])).unwrap();
        assert_eq!(ok.camera, Some(0));
        let both = parse_options(args(&[
            "--resolution",
            "160x120",
            "--camera",
            "/dev/video12",
        ]))
        .unwrap();
        assert_eq!((both.camera, both.low_resolution), (Some(12), true));
        assert_eq!(
            parse_options(args(&["--camera", "/dev/video65535"]))
                .unwrap()
                .camera,
            Some(65535)
        );
        assert_eq!(parse_options(args(&[])).unwrap().camera, None);
        for values in [
            vec!["--camera"],
            vec!["--camera", ""],
            vec!["--camera", "0"],
            vec!["--camera", "video0"],
            vec!["--camera", "dev/video0"],
            vec!["--camera", "./dev/video0"],
            vec!["--camera", "/dev/video"],
            vec!["--camera", "/dev/video00"],
            vec!["--camera", "/dev/video01"],
            vec!["--camera", "/dev/video-1"],
            vec!["--camera", "/dev/video+1"],
            vec!["--camera", "/dev/video1a"],
            vec!["--camera", "/dev/video1/"],
            vec!["--camera", "/dev/video65536"],
            vec!["--camera", "/dev/video123456"],
            vec!["--camera", "/dev/../dev/video0"],
            vec!["--camera", "/dev/video0 "],
            vec!["--camera", "/dev/video0", "--camera", "/dev/video1"],
            vec!["--camera", "/dev/video0", "--camera", "/dev/video0"],
            vec!["--camera", "/dev/video0", "--self-test"],
            vec!["--self-test", "--camera", "/dev/video0"],
            vec!["--camera", "/dev/video0", "--help"],
            vec!["--help", "--camera", "/dev/video0"],
            vec!["--camera", "--fit", "cover"],
        ] {
            assert!(parse_options(args(&values)).is_err(), "{values:?}");
        }
        assert!(parse_options(args(&["--camera", "/dev/video0\u{0}x"])).is_err());
        assert!(HELP.contains("--camera") && HELP.contains("sample only"));
    }

    #[test]
    fn metrics_and_run_for_arguments_are_strict_and_optional() {
        assert_eq!(parse_options(args(&[])).unwrap(), Options::default());
        assert!(!Options::default().metrics && Options::default().run_for_ms.is_none());
        let on = parse_options(args(&["--metrics", "--run-for-ms", "100"])).unwrap();
        assert!(on.metrics && on.run_for_ms == Some(100));
        assert_eq!(
            parse_options(args(&["--run-for-ms", "600000", "--camera", "/dev/video2"]))
                .unwrap()
                .run_for_ms,
            Some(600_000)
        );
        for values in [
            vec!["--metrics", "--metrics"],
            vec!["--run-for-ms"],
            vec!["--run-for-ms", "99"],
            vec!["--run-for-ms", "600001"],
            vec!["--run-for-ms", "0"],
            vec!["--run-for-ms", "0100"],
            vec!["--run-for-ms", "-100"],
            vec!["--run-for-ms", "+100"],
            vec!["--run-for-ms", "1e3"],
            vec!["--run-for-ms", "100.0"],
            vec!["--run-for-ms", "NaN"],
            vec!["--run-for-ms", ""],
            vec!["--run-for-ms", "1000000"],
            vec!["--run-for-ms", "99999999999999999999"],
            vec!["--run-for-ms", "100", "--run-for-ms", "100"],
            vec!["--self-test", "--metrics"],
            vec!["--metrics", "--self-test"],
            vec!["--self-test", "--run-for-ms", "100"],
            vec!["--run-for-ms", "100", "--self-test"],
            vec!["--metrics", "--help"],
            vec!["--help", "--metrics"],
            vec!["--help", "--run-for-ms", "100"],
            vec!["--run-for-ms", "100", "--help"],
        ] {
            assert!(parse_options(args(&values)).is_err(), "{values:?}");
        }
        assert!(HELP.contains("--metrics") && HELP.contains("--run-for-ms"));
    }

    mod pipeline {
        use super::*;
        use crate::camera::testing::{wait_until, FakeDevice, Log, ManualClock, Step};
        use crate::camera::{CaptureError, Offer, Opener};
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

        const CLEAR: TemporalOptions = TemporalOptions {
            delay_ms: 0,
            trail_strength: 0,
        };
        const RED: u32 = 0xff0000;

        struct Rig {
            clock: Arc<ManualClock>,
            log: Arc<Log>,
            gate: Arc<AtomicUsize>,
        }

        fn rig() -> Rig {
            Rig {
                clock: Arc::new(ManualClock::default()),
                log: Arc::new(Log::default()),
                gate: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn rgb24(width: u32, height: u32, rig: &Rig) -> FakeDevice {
            let mut device = FakeDevice::with_format(
                *b"RGB3",
                width,
                height,
                width * 3 + 12,
                &rig.clock,
                &rig.log,
            );
            device.gate = Some(Arc::clone(&rig.gate));
            device
        }

        fn start_with(open: Opener, width: usize, height: usize, rig: &Rig) -> Pipeline {
            let session = CameraSession::spawn(
                open,
                (width, height),
                Arc::clone(&rig.clock) as Arc<dyn Clock>,
            )
            .unwrap();
            Pipeline::new(Sample::new(width, height).unwrap(), Some(session))
        }

        fn start(device: FakeDevice, width: usize, height: usize, rig: &Rig) -> Pipeline {
            start_with(device.open(), width, height, rig)
        }

        /// RGB24 payload with 12 padding bytes (0xEE) per row.
        fn solid(width: usize, height: usize, rgb: [u8; 3]) -> Vec<u8> {
            let stride = width * 3 + 12;
            let mut raw = vec![0xEE; stride * height];
            for y in 0..height {
                for x in 0..width {
                    raw[y * stride + x * 3..][..3].copy_from_slice(&rgb);
                }
            }
            raw
        }

        fn data(at_ms: u64, sequence: u32, bytes: Vec<u8>) -> Step {
            Step::Data {
                at_ms,
                sequence,
                bytes,
            }
        }

        /// Releases one scripted frame and polls until the pipeline shows it.
        fn deliver(
            pipeline: &mut Pipeline,
            rig: &Rig,
            now: u64,
            options: TemporalOptions,
            reduced: bool,
        ) {
            let before = pipeline.camera_frames;
            rig.gate.fetch_add(1, SeqCst);
            wait_until(|| {
                pipeline.update(now, options, reduced).unwrap();
                pipeline.camera_frames > before
            });
        }

        /// Polls until the pipeline reports a source change.
        fn notice(pipeline: &mut Pipeline, now: u64, options: TemporalOptions) -> String {
            let mut found = None;
            wait_until(|| {
                found = pipeline.update(now, options, false).unwrap();
                found.is_some()
            });
            found.unwrap()
        }

        #[test]
        fn default_sample_output_is_unchanged_by_the_pipeline() {
            for (w, h) in [(160, 120), (320, 240)] {
                let mut direct = Sample::new(w, h).unwrap();
                let mut pipeline = Pipeline::new(Sample::new(w, h).unwrap(), None);
                assert_eq!(pipeline.source, Source::Sample);
                let options = Options::default().temporal;
                for time in [0, 40, 130, 500, 900] {
                    direct.update(time, options, false).unwrap();
                    assert_eq!(pipeline.update(time, options, false), Ok(None));
                    assert_eq!(pipeline.pixels(), direct.pixels());
                }
            }
        }

        #[test]
        fn camera_pixels_are_stride_aware_and_mirrored_once_at_both_sizes() {
            for (w, h) in [(320usize, 240usize), (160, 120)] {
                let rig = rig();
                let (source_w, source_h) = (320usize, 240usize);
                let stride = source_w * 3 + 12;
                let mut raw = vec![0xEE; stride * source_h];
                for y in 0..source_h {
                    for x in 0..source_w {
                        raw[y * stride + x * 3..][..3].copy_from_slice(&[
                            (x % 256) as u8,
                            y as u8,
                            7,
                        ]);
                    }
                }
                let mut device = rgb24(source_w as u32, source_h as u32, &rig);
                device.script.push_back(data(10, 1, raw));
                let mut pipeline = start(device, w, h, &rig);
                deliver(&mut pipeline, &rig, 20, CLEAR, false);
                assert_eq!(pipeline.source, Source::Camera);
                let pixels = pipeline.pixels();
                if (w, h) == (320, 240) {
                    // Source column 319 (r = 63) lands at x = 0; column 0 at x = 319.
                    assert_eq!(pixels[0], 0x3f0007);
                    assert_eq!(pixels[319], 0x000007);
                    assert_eq!(pixels[239 * 320], 0x3fef07);
                    assert_eq!(pixels[239 * 320 + 319], 0x00ef07);
                } else {
                    // 2 x 2 block means: r (62+63+62+63+2)/4 = 63, g (0+0+1+1+2)/4 = 1.
                    assert_eq!(pixels[0], 0x3f0107);
                    // Columns 0,1: r (0+1+0+1+2)/4 = 1.
                    assert_eq!(pixels[159], 0x010107);
                    // Rows 238,239: g (238+238+239+239+2)/4 = 239.
                    assert_eq!(pixels[119 * 160], 0x3fef07);
                    assert_eq!(pixels[119 * 160 + 159], 0x01ef07);
                }
                assert_eq!(pixels.len(), w * h);
            }
        }

        #[test]
        fn yuyv_camera_frames_use_the_documented_bt601_colors() {
            let rig = rig();
            let mut device = FakeDevice::yuyv(16, 12, &rig.clock, &rig.log);
            device.gate = Some(Arc::clone(&rig.gate));
            // Left 8 columns red (Y81 U90 V240), right 8 columns blue (Y41 U240 V110).
            let mut raw = Vec::new();
            for _ in 0..12 {
                for pair in 0..8 {
                    raw.extend_from_slice(if pair < 4 {
                        &[81, 90, 81, 240]
                    } else {
                        &[41, 240, 41, 110]
                    });
                }
            }
            device.script.push_back(data(10, 1, raw));
            let mut pipeline = start(device, 16, 12, &rig);
            deliver(&mut pipeline, &rig, 10, CLEAR, false);
            // Mirrored once: the blue half now appears on the left.
            assert_eq!(pipeline.pixels()[0], 0x0000ff);
            assert_eq!(pipeline.pixels()[7], 0x0000ff);
            assert_eq!(pipeline.pixels()[8], RED);
            assert_eq!(pipeline.pixels()[15], RED);
        }

        #[test]
        fn display_ticks_do_not_reprocess_the_same_frame() {
            let rig = rig();
            let mut device = rgb24(16, 12, &rig);
            device
                .script
                .push_back(data(10, 1, solid(16, 12, [255, 0, 0])));
            let mut pipeline = start(device, 16, 12, &rig);
            deliver(&mut pipeline, &rig, 10, CLEAR, false);
            let shown = pipeline.pixels().to_vec();
            for tick in 11..20 {
                assert_eq!(pipeline.update(tick, CLEAR, false), Ok(None));
            }
            assert_eq!((pipeline.camera_frames, pipeline.pixels()), (1, &shown[..]));
        }

        #[test]
        fn camera_reduced_motion_stays_live_without_delay_or_trails() {
            let blend = TemporalOptions {
                delay_ms: 0,
                trail_strength: 128,
            };
            let mut shown = Vec::new();
            for reduced in [false, true] {
                let rig = rig();
                let mut device = rgb24(16, 12, &rig);
                device
                    .script
                    .push_back(data(1000, 1, solid(16, 12, [255, 0, 0])));
                device
                    .script
                    .push_back(data(1100, 2, solid(16, 12, [0, 0, 255])));
                let mut pipeline = start(device, 16, 12, &rig);
                deliver(&mut pipeline, &rig, 1000, blend, reduced);
                assert!(pipeline.pixels().iter().all(|&p| p == RED));
                deliver(&mut pipeline, &rig, 1100, blend, reduced);
                assert_eq!(pipeline.source, Source::Camera);
                assert_eq!(pipeline.camera_frames, 2);
                assert!(pipeline.pixels().iter().all(|&p| p == pipeline.pixels()[0]));
                shown.push(pipeline.pixels()[0]);
            }
            // Trails: (0*127 + 255*128 + 127)/255 = 128 red, (255*127 + 127)/255 = 127 blue.
            assert_eq!(shown, [0x80007f, 0x0000ff]);
        }

        #[test]
        fn stall_falls_back_once_clears_history_and_releases_the_device() {
            let rig = rig();
            let options = TemporalOptions {
                delay_ms: 250,
                trail_strength: 0,
            };
            let mut device = rgb24(16, 12, &rig);
            device
                .script
                .push_back(data(1000, 1, solid(16, 12, [255, 0, 0])));
            let mut pipeline = start(device, 16, 12, &rig);
            deliver(&mut pipeline, &rig, 1000, options, false);
            assert!(pipeline.pixels().iter().all(|&p| p == RED));
            // The last complete frame was at 1000 ms; 2000 ms is still within the bound.
            assert_eq!(pipeline.update(2000, options, false), Ok(None));
            assert_eq!(pipeline.source, Source::Camera);

            let message = pipeline.update(2101, options, false).unwrap().unwrap();
            assert!(message.contains("more than 1 second"), "{message}");
            assert!(message.contains("using the generated sample"), "{message}");
            assert_eq!(pipeline.source, Source::Sample);
            let mut fresh = Sample::new(16, 12).unwrap();
            fresh.update(2101, options, false).unwrap();
            assert!(
                pipeline.pixels().iter().all(|&p| p != RED),
                "camera pixels leaked"
            );
            assert_eq!(pipeline.pixels(), fresh.pixels(), "history was not cleared");

            // No second notice and no reopen: the sample simply continues.
            fresh.update(2200, options, false).unwrap();
            assert_eq!(pipeline.update(2200, options, false), Ok(None));
            assert_eq!(pipeline.pixels(), fresh.pixels());
            wait_until(|| {
                pipeline.update(2200, options, false).unwrap();
                pipeline.retiring.is_none()
            });
            assert!(pipeline.camera.is_none());
            assert_eq!(rig.log.opened.load(SeqCst), 1);
            assert_eq!(
                (rig.log.stops.load(SeqCst), rig.log.dropped.load(SeqCst)),
                (1, 1)
            );
        }

        #[test]
        fn stale_camera_frames_are_not_displayed() {
            for (now, shown) in [(1500, 1), (1501, 0)] {
                let rig = rig();
                let mut device = rgb24(16, 12, &rig);
                device
                    .script
                    .push_back(data(1000, 1, solid(16, 12, [255, 0, 0])));
                let mut pipeline = start(device, 16, 12, &rig);
                rig.gate.fetch_add(1, SeqCst);
                wait_until(|| rig.log.waits.load(SeqCst) >= 2);
                pipeline.update(now, CLEAR, false).unwrap();
                assert_eq!(pipeline.camera_frames, shown, "now = {now}");
                assert_eq!(pipeline.source, Source::Camera);
            }
        }

        #[test]
        fn unusable_cameras_fall_back_with_a_clear_notice_and_a_usable_sample() {
            let options = Options::default().temporal;
            let cases: Vec<(&str, Opener)> = vec![
                (
                    "permission denied",
                    Box::new(|| Err(CaptureError::Permission)),
                ),
                ("not found", Box::new(|| Err(CaptureError::Missing))),
                ("MJPEG decoding is not supported", {
                    let rig = rig();
                    let mut device = FakeDevice::yuyv(640, 480, &rig.clock, &rig.log);
                    device.offers = Ok(vec![Offer {
                        fourcc: *b"MJPG",
                        sizes: vec![(640, 480)],
                    }]);
                    device.open()
                }),
                ("no 4:3", {
                    let rig = rig();
                    let mut device = FakeDevice::yuyv(160, 120, &rig.clock, &rig.log);
                    device.offers = Ok(vec![Offer {
                        fourcc: *b"YUYV",
                        sizes: vec![(160, 120)],
                    }]);
                    device.open()
                }),
            ];
            for (expected, open) in cases {
                let rig = rig();
                let mut pipeline = start_with(open, 320, 240, &rig);
                let message = notice(&mut pipeline, 5, options);
                assert!(message.contains(expected), "{expected}: {message}");
                assert_eq!(pipeline.source, Source::Sample);
                let mut direct = Sample::new(320, 240).unwrap();
                direct.update(5, options, false).unwrap();
                assert_eq!(pipeline.pixels(), direct.pixels());
            }
        }

        #[test]
        fn startup_timeout_disconnect_and_eof_fall_back() {
            // Startup: no frame ever arrives.
            let rig_a = rig();
            let mut pipeline = start(rgb24(16, 12, &rig_a), 16, 12, &rig_a);
            assert_eq!(pipeline.update(2000, CLEAR, false), Ok(None));
            let message = pipeline.update(2001, CLEAR, false).unwrap().unwrap();
            assert!(message.contains("2 seconds"), "{message}");
            // Disconnect and end of stream after a first frame.
            for (error, text) in [
                (CaptureError::Disconnected, "disconnected"),
                (CaptureError::Eof, "ended"),
            ] {
                let rig = rig();
                let mut device = rgb24(16, 12, &rig);
                device
                    .script
                    .push_back(data(10, 1, solid(16, 12, [1, 2, 3])));
                device.script.push_back(Step::Fail(error));
                let mut pipeline = start(device, 16, 12, &rig);
                rig.gate.fetch_add(2, SeqCst);
                let mut messages = Vec::new();
                wait_until(|| {
                    messages.extend(pipeline.update(10, CLEAR, false).unwrap());
                    pipeline.source == Source::Sample
                });
                assert!(messages.iter().any(|m| m.contains(text)), "{messages:?}");
            }
        }

        #[test]
        fn escape_and_window_close_cancel_join_and_close_the_device() {
            for explicit in [true, false] {
                let rig = rig();
                let device = rgb24(16, 12, &rig);
                let mut pipeline = start(device, 16, 12, &rig);
                wait_until(|| rig.log.waits.load(SeqCst) >= 2);
                if explicit {
                    pipeline.shutdown(); // loop exit after Escape or window close
                    assert!(pipeline.camera.is_none() && pipeline.retiring.is_none());
                } else {
                    drop(pipeline); // error return through the loop
                }
                assert_eq!(
                    (rig.log.stops.load(SeqCst), rig.log.dropped.load(SeqCst)),
                    (1, 1)
                );
            }
        }

        #[test]
        fn shutdown_joins_a_retiring_worker_after_fallback() {
            let rig = rig();
            let mut pipeline = start(rgb24(16, 12, &rig), 16, 12, &rig);
            wait_until(|| rig.log.started.load(SeqCst) == 1);
            assert!(pipeline.update(2001, CLEAR, false).unwrap().is_some());
            pipeline.shutdown();
            assert_eq!(
                (rig.log.stops.load(SeqCst), rig.log.dropped.load(SeqCst)),
                (1, 1)
            );
        }

        struct Tick(std::sync::atomic::AtomicU64);
        impl MicroClock for Tick {
            /// Every reading advances 100 us, so spans are exactly 100 us.
            fn now_us(&self) -> u64 {
                self.0.fetch_add(100, SeqCst)
            }
        }

        fn recorder(camera: bool) -> Recorder {
            let mut recorder = Recorder::new(
                Arc::new(Tick(std::sync::atomic::AtomicU64::new(0))),
                RunConfig {
                    width: 16,
                    height: 12,
                    fit: "cover",
                    delay_ms: 0,
                    trails: 0,
                    reduced_motion: false,
                    camera_requested: camera,
                    run_for_ms: None,
                },
            );
            recorder.mark_loop_start();
            recorder
        }

        /// One display iteration as the main loop performs it.
        fn tick(pipeline: &mut Pipeline, recorder: &mut Recorder, now: u64) {
            let (_, span) = update_measured(pipeline, Some(recorder), now, CLEAR, false).unwrap();
            let (start, end) = span.unwrap();
            let prepare_end = recorder.now_us();
            let submit_end = recorder.now_us();
            recorder.record(Iteration {
                source: pipeline.source_kind(),
                step: pipeline.last_step,
                step_start_us: start,
                step_end_us: end,
                prepare_end_us: prepare_end,
                submit_end_us: submit_end,
            });
        }

        #[test]
        fn measurement_does_not_change_sample_pixels_at_both_sizes() {
            for (w, h) in [(160, 120), (320, 240)] {
                let options = Options::default().temporal;
                let mut plain = Pipeline::new(Sample::new(w, h).unwrap(), None);
                let mut measured = Pipeline::new(Sample::new(w, h).unwrap(), None);
                let mut rec = recorder(false);
                for time in [0, 40, 130, 500, 900] {
                    plain.update(time, options, false).unwrap();
                    let (_, span) =
                        update_measured(&mut measured, Some(&rec), time, options, false).unwrap();
                    assert_eq!(span, Some((span.unwrap().0, span.unwrap().0 + 100)));
                    assert_eq!(measured.pixels(), plain.pixels());
                    rec.record(Iteration {
                        source: measured.source_kind(),
                        step: measured.last_step,
                        step_start_us: 0,
                        step_end_us: 1,
                        prepare_end_us: 2,
                        submit_end_us: time,
                    });
                }
                // Without a recorder no span is produced.
                assert_eq!(
                    update_measured(&mut plain, None, 1000, options, false)
                        .unwrap()
                        .1,
                    None
                );
            }
        }

        #[test]
        fn measuring_camera_updates_preserves_pixels() {
            let plain_rig = rig();
            let measured_rig = rig();
            let make_device = |rig: &Rig| {
                let mut device = rgb24(16, 12, rig);
                for (time, sequence, color) in [(10, 1, [255, 0, 0]), (20, 2, [0, 0, 255])] {
                    device
                        .script
                        .push_back(data(time, sequence, solid(16, 12, color)));
                }
                device
            };
            let mut plain = start(make_device(&plain_rig), 16, 12, &plain_rig);
            let mut measured = start(make_device(&measured_rig), 16, 12, &measured_rig);
            let mut rec = recorder(true);
            for time in [10, 20] {
                deliver(&mut plain, &plain_rig, time, CLEAR, false);
                let before = measured.camera_frames;
                measured_rig.gate.fetch_add(1, SeqCst);
                wait_until(|| {
                    tick(&mut measured, &mut rec, time);
                    measured.camera_frames > before
                });
                assert_eq!(plain.pixels(), measured.pixels());
            }
        }

        #[test]
        fn idle_display_ticks_never_become_camera_frames() {
            let rig = rig();
            let mut device = rgb24(16, 12, &rig);
            device
                .script
                .push_back(data(10, 1, solid(16, 12, [255, 0, 0])));
            let mut pipeline = start(device, 16, 12, &rig);
            let mut rec = recorder(true);
            rig.gate.fetch_add(1, SeqCst);
            wait_until(|| {
                tick(&mut pipeline, &mut rec, 10);
                pipeline.camera_frames == 1
            });
            let before = pipeline.camera_frames;
            for now in 11..21 {
                tick(&mut pipeline, &mut rec, now);
                assert_eq!(pipeline.last_step, metrics::Step::Idle);
            }
            assert_eq!(pipeline.camera_frames, before);
            let text = rec.report(ExitReason::Timed, 100_000, 100_100);
            assert!(text.contains("\"frames\":1,"), "{text}");
            let camera = text.split("\"camera\":").nth(1).unwrap();
            let count = |key: &str| -> u64 {
                camera
                    .split(key)
                    .nth(1)
                    .unwrap()
                    .split(',')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            };
            let submissions = count("\"submissions\":");
            assert!(submissions >= 11);
            assert_eq!(count("\"idle_ticks\":"), submissions - 1);
            assert!(text.contains("\"processing_us\":{\"count\":1,"), "{text}");
        }

        #[test]
        fn capture_counters_survive_fallback_and_orderly_shutdown() {
            // Fallback: stall after two published frames, one unread replaced.
            let rig = rig();
            let mut device = rgb24(16, 12, &rig);
            for (at, sequence) in [(1000, 1), (1001, 2), (1002, 3)] {
                device
                    .script
                    .push_back(data(at, sequence, solid(16, 12, [1, 2, 3])));
            }
            let mut pipeline = start(device, 16, 12, &rig);
            rig.gate.fetch_add(3, SeqCst);
            wait_until(|| rig.log.waits.load(SeqCst) >= 4);
            // The first frame also announces the negotiated format.
            let message = pipeline.update(1002, CLEAR, false).unwrap();
            assert!(message.unwrap().starts_with("camera: "));
            assert_eq!(pipeline.camera_frames, 1);
            assert!(pipeline.update(2400, CLEAR, false).unwrap().is_some());
            wait_until(|| {
                pipeline.update(2400, CLEAR, false).unwrap();
                pipeline.retiring.is_none()
            });
            assert!(pipeline.camera.is_none());
            let counters = pipeline.capture.expect("snapshot kept after release");
            assert_eq!(counters.published, 3);
            assert_eq!(counters.overwritten, 2);
            assert_eq!(counters.stale, 0);
            // The sample continues and its counts are separate.
            assert_eq!(pipeline.source_kind(), SourceKind::Sample);

            // Orderly shutdown while streaming keeps the counters too.
            let rig2 = self::rig();
            let mut device = rgb24(16, 12, &rig2);
            device
                .script
                .push_back(data(10, 1, solid(16, 12, [9, 9, 9])));
            let mut live = start(device, 16, 12, &rig2);
            rig2.gate.fetch_add(1, SeqCst);
            wait_until(|| {
                live.update(10, CLEAR, false).unwrap();
                live.camera_frames == 1
            });
            live.shutdown();
            assert_eq!(live.capture.map(|c| c.published), Some(1));
            assert_eq!(
                (rig2.log.stops.load(SeqCst), rig2.log.dropped.load(SeqCst)),
                (1, 1)
            );
        }

        #[test]
        fn counters_are_unavailable_when_the_camera_never_streamed() {
            let rig = rig();
            let mut pipeline = start_with(Box::new(|| Err(CaptureError::Missing)), 16, 12, &rig);
            let mut rec = recorder(true);
            wait_until(|| {
                tick(&mut pipeline, &mut rec, 5);
                pipeline.source == Source::Sample
            });
            wait_until(|| {
                pipeline.update(5, CLEAR, false).unwrap();
                pipeline.retiring.is_none()
            });
            assert_eq!(pipeline.capture, None);
            rec.set_capture(pipeline.capture);
            let text = rec.report(ExitReason::Timed, 100_000, 100_100);
            assert!(text.contains("\"capture\":null"));
            assert!(text.contains("\"fallback_occurred\":true"));
            assert!(text.contains("\"frames\":0,"));
        }
    }
}
