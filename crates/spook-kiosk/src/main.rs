use minifb::{Key, Window, WindowOptions};
use spook_core::{copy_mirrored_rgb24, RgbFrame};
use std::error::Error;

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
    window.set_target_fps(30);
    while window.is_open() && !window.is_key_down(Key::Escape) {
        window.update_with_buffer(&output, width, height)?;
        if proof {
            println!("Rendered a synthetic frame through the X11 software window.");
            break;
        }
    }
    Ok(())
}
