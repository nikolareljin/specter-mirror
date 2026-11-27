use minifb::{InputCallback, Key, Window, WindowOptions};
use spook_core::{TemporalError, TemporalOptions};
use std::cell::Cell;
use std::error::Error;
use std::rc::Rc;
use std::sync::Arc;

mod camera;
mod frame;
mod latest_frame;
mod presentation;
mod sample;
use camera::{CameraPoll, CameraSession, Clock, MonotonicClock};
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
The appliance session requests fullscreen. Camera mode is opt-in, needs read/write\n\
access to the device and has no hardware verification yet.";

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
    if options.proof && options.camera.is_some() {
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
        if self.retiring.as_mut().is_some_and(CameraSession::try_reap) {
            self.retiring = None;
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
        Ok(notice)
    }

    /// Switches to the sample for good and clears every camera pixel from
    /// the shared history before the sample writes into it.
    fn fall_back(&mut self, reason: &str) -> String {
        if let Some(camera) = self.camera.take() {
            camera.cancel();
            self.retiring = Some(camera);
        }
        self.source = Source::Sample;
        self.sample.reset();
        format!("camera: {reason}; using the generated sample")
    }

    /// Cancels and joins every worker, closing the device.
    fn shutdown(&mut self) {
        self.camera = None;
        self.retiring = None;
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
    let result = (|| -> Result<(), Box<dyn Error>> {
        while window.is_open() && !exit_requested.get() {
            let (display_width, display_height) = window.get_size();
            if display_width == 0 || display_height == 0 {
                window.update();
                continue;
            }
            if let Some(notice) =
                pipeline.update(clock.now_ms(), options.temporal, options.reduced_motion)?
            {
                eprintln!("{notice}");
            }
            presentation.update(
                pipeline.pixels(),
                display_width,
                display_height,
                options.fit,
            )?;
            let crop = presentation.crop();
            window.update_with_buffer(presentation.pixels(), crop.width, crop.height)?;
            if options.proof {
                println!("Rendered a synthetic frame through the X11 software window.");
                break;
            }
        }
        Ok(())
    })();
    // Escape, window close and errors all end here: cancel, join, close.
    pipeline.shutdown();
    result
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
    }
}
