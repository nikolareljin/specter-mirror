use minifb::{InputCallback, Key, Window, WindowOptions};
use spook_core::{copy_mirrored_rgb24, RgbFrame};
use std::cell::Cell;
use std::error::Error;
use std::rc::Rc;

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
    let mut arguments = std::env::args_os().skip(1);
    let proof = match arguments.next() {
        None => false,
        Some(flag) if flag == "--self-test" => true,
        _ => return Err("usage: spook-kiosk [--self-test]".into()),
    };
    if arguments.next().is_some() {
        return Err("usage: spook-kiosk [--self-test]".into());
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
            scale_mode: minifb::ScaleMode::AspectRatioStretch,
            ..WindowOptions::default()
        },
    )?;
    let exit_requested = Rc::new(Cell::new(false));
    window.set_input_callback(Box::new(ExitKeys(Rc::clone(&exit_requested))));
    window.set_target_fps(30);
    while window.is_open() && !exit_requested.get() {
        window.update_with_buffer(&output, width, height)?;
        if proof {
            println!("Rendered a synthetic frame through the X11 software window.");
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
