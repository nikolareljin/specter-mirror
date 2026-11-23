use minifb::{InputCallback, Key, Window, WindowOptions};
use spook_core::{copy_mirrored_rgb24, RgbFrame};
use std::cell::Cell;
use std::error::Error;
use std::rc::Rc;

mod presentation;
use presentation::{FitMode, Presentation};

const HELP: &str = "Usage: spook-kiosk [--self-test] [--fit cover|contain|stretch]\n\
Display a synthetic Rust frame through CPU-only X11. Escape exits.\n\
  --fit cover     Fill the window, cropping the center (default).\n\
  --fit contain   Preserve the complete image with letterboxing.\n\
  --fit stretch   Fill the window without preserving aspect ratio.\n\
  --self-test     Render one frame, then exit.\n\
  -h, --help      Print help without opening a window.\n\
Examples:\n\
  spook-kiosk\n\
  spook-kiosk --fit contain\n\
  spook-kiosk --self-test --fit cover\n\
The appliance session requests fullscreen. No webcam capture is implemented.";

#[derive(Debug, Default, PartialEq, Eq)]
struct Options {
    proof: bool,
    help: bool,
    fit: FitMode,
}

fn parse_options(
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Options, &'static str> {
    let mut options = Options::default();
    let mut fit_seen = false;
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
            Some("--help" | "-h") if !options.proof && !fit_seen && arguments.peek().is_none() => {
                options.help = true;
            }
            _ => return Err(HELP),
        }
    }
    Ok(options)
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
    let (width, height) = (320, 240);
    let mut source = vec![0u8; width * height * 3];
    for y in 0..height {
        for x in 0..width {
            let offset = (y * width + x) * 3;
            source[offset] = (x % 256) as u8;
            source[offset + 1] = (y % 256) as u8;
            source[offset + 2] = 96;
        }
    }
    let frame = RgbFrame::new(width, height, width * 3, &source)?;
    let mut output = vec![0; width * height];
    copy_mirrored_rgb24(&frame, &mut output)?;
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
    while window.is_open() && !exit_requested.get() {
        let (display_width, display_height) = window.get_size();
        if display_width == 0 || display_height == 0 {
            window.update();
            continue;
        }
        presentation.update(&output, display_width, display_height, options.fit)?;
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
                    fit
                }
            );
            assert_eq!(
                parse_options(args(&["--fit", name, "--self-test"])).unwrap(),
                options
            );
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
