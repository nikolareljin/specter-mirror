use minifb::{InputCallback, Key, Window, WindowOptions};
use spook_core::TemporalOptions;
use std::cell::Cell;
use std::error::Error;
use std::rc::Rc;
use std::time::Instant;

mod presentation;
mod sample;
use presentation::{FitMode, Presentation};
use sample::Sample;

const HELP: &str = "Usage: spook-kiosk [options]\n\
Display an animated generated sample through CPU-only X11. Escape exits.\n\
  --fit cover     Fill the window, cropping the center (default).\n\
  --fit contain   Preserve the complete image with letterboxing.\n\
  --fit stretch   Fill the window without preserving aspect ratio.\n\
  --self-test     Render one frame, then exit.\n\
  --delay-ms N    Reflection delay, 0..600 ms (default 250).\n\
  --trails N      Past-frame blend weight, 0..255 (default 115).\n\
  --resolution S Processing size: 320x240 (default) or 160x120.\n\
  --reduced-motion  Freeze the sample and disable delay/trails.\n\
  -h, --help      Print help without opening a window.\n\
Examples:\n\
  spook-kiosk\n\
  spook-kiosk --fit contain\n\
  spook-kiosk --resolution 160x120 --delay-ms 400 --trails 128\n\
  spook-kiosk --reduced-motion\n\
  spook-kiosk --self-test --fit cover\n\
The appliance session requests fullscreen. No webcam capture is implemented.";

#[derive(Debug, PartialEq, Eq)]
struct Options {
    proof: bool,
    help: bool,
    fit: FitMode,
    temporal: TemporalOptions,
    low_resolution: bool,
    reduced_motion: bool,
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
    Ok(options)
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
    let mut sample = Sample::new(width, height)?;
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
    let clock = Instant::now();
    while window.is_open() && !exit_requested.get() {
        let (display_width, display_height) = window.get_size();
        if display_width == 0 || display_height == 0 {
            window.update();
            continue;
        }
        let now_ms = clock.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        sample.update(now_ms, options.temporal, options.reduced_motion)?;
        presentation.update(sample.pixels(), display_width, display_height, options.fit)?;
        let crop = presentation.crop();
        window.update_with_buffer(presentation.pixels(), crop.width, crop.height)?;
        if options.proof {
            println!("Rendered a synthetic frame through the X11 software window.");
            break;
        }
    }
    Ok(())
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
}
