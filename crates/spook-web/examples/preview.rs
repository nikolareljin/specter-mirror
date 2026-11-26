//! Desktop preview of the browser engine with a sample or optional webcam.
//!
//! Runs the real `MirrorEngine` on a generated moving visitor. By default it
//! opens an X11 window and shows the mirror live; `--save` writes PNG frames
//! and a viewer page, and `--no-frames` prints only a timeline. Run it with
//! `./dev preview --help`.
//!
//! The visitor is a Rust approximation of the page's generated sample scene
//! (same pose cycle and head ellipse, simpler drawing). Its head box comes
//! from the scene geometry, the same way the browser sample supplies it. It is
//! not face detection. Output is a development aid, not a performance or
//! hardware measurement.
#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use spook_web::MirrorEngine;
#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
use std::time::{Duration, Instant};
use std::{f64::consts::PI, fs, path::PathBuf, process::ExitCode};

const CYCLE_MS: f64 = 28_000.0;
const USAGE: &str = "\
Usage: preview [options]
By default a window opens and shows the mirror live (needs an X11 DISPLAY).
  --seed N          Timing seed, 0..4294967295 (default 1)
  --speed N         Live clock multiplier, 1..60 (default 1)
  --scale N         Live window zoom: 1, 2 or 4 (default 2)
  --seconds S       Stop after S seconds, 1..3600 (live: until closed;
                    saved runs: 60, at most 600)
  --size WxH        320x240 or 160x120 (default 320x240)
  --no-skull        Start with skull apparitions off
  --reduced-motion  Enable reduced motion
  --short-visitor   Use the original 28 second visitor loop
  --camera [DEV]    Live window from a webcam (default /dev/video0) via ffmpeg;
                    a local face detector places the skull over a frontal face.
  --show-face       With --camera, outline each detected face box in green
  --save            No window: write PNG frames and an index.html viewer
  --open            Like --save, then open the viewer with xdg-open
  --every MS        With --save, a frame every MS ms, multiple of 100 (200)
  --out DIR         With --save, existing empty output directory;
                    default target/preview is created; '..' is rejected
  --no-frames       No window or files: print the timeline only
  -h, --help        Show this text
Live keys: Space pauses, K toggles the skull, Escape or Q quits.";

struct Options {
    seed: u32,
    seconds: Option<u64>,
    speed: u64,
    scale: u32,
    every: u64,
    width: u32,
    height: u32,
    out: PathBuf,
    skull: bool,
    reduced: bool,
    mode: Mode,
    fixed_visitor: bool,
    open: bool,
    camera: Option<String>,
    show_face: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Live,
    Save,
    Timeline,
}

fn parse() -> Result<Option<Options>, String> {
    let mut o = Options {
        seed: 1,
        seconds: None,
        speed: 1,
        scale: 2,
        every: 200,
        width: 320,
        height: 240,
        out: PathBuf::from("target/preview"),
        skull: true,
        reduced: false,
        mode: Mode::Live,
        fixed_visitor: false,
        open: false,
        camera: None,
        show_face: false,
    };
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(None),
            "--seed" => o.seed = value("--seed")?.parse().map_err(|_| "invalid --seed")?,
            "--seconds" => {
                let seconds = value("--seconds")?
                    .parse()
                    .map_err(|_| "invalid --seconds")?;
                if !(1..=3600).contains(&seconds) {
                    return Err("--seconds must be 1..3600".into());
                }
                o.seconds = Some(seconds);
            }
            "--speed" => {
                o.speed = value("--speed")?.parse().map_err(|_| "invalid --speed")?;
                if !(1..=60).contains(&o.speed) {
                    return Err("--speed must be 1..60".into());
                }
            }
            "--scale" => {
                o.scale = value("--scale")?.parse().map_err(|_| "invalid --scale")?;
                if ![1, 2, 4].contains(&o.scale) {
                    return Err("--scale must be 1, 2 or 4".into());
                }
            }
            "--every" => {
                o.every = value("--every")?.parse().map_err(|_| "invalid --every")?;
                if o.every == 0 || !o.every.is_multiple_of(100) {
                    return Err("--every must be a positive multiple of 100".into());
                }
            }
            "--size" => {
                (o.width, o.height) = match value("--size")?.as_str() {
                    "320x240" => (320, 240),
                    "160x120" => (160, 120),
                    _ => return Err("--size must be 320x240 or 160x120".into()),
                }
            }
            "--out" => o.out = output_directory(&value("--out")?)?,
            "--no-skull" => o.skull = false,
            "--reduced-motion" => o.reduced = true,
            "--no-frames" => o.mode = Mode::Timeline,
            "--save" => o.mode = Mode::Save,
            "--short-visitor" => o.fixed_visitor = true,
            "--show-face" => o.show_face = true,
            "--camera" => {
                let device = match args.peek() {
                    Some(next) if !next.starts_with('-') => args.next().unwrap_or_default(),
                    _ => "/dev/video0".into(),
                };
                if device.strip_prefix("/dev/video").is_none_or(|suffix| {
                    suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit())
                }) {
                    return Err("--camera expects a /dev/videoN device".into());
                }
                o.camera = Some(device);
            }
            "--open" => {
                o.open = true;
                o.mode = Mode::Save;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    if o.camera.is_some() && o.speed != 1 {
        return Err(
            "--speed cannot be used with --camera: face detections take real \
                    time, and a faster clock would make every one too old to use"
                .into(),
        );
    }
    if o.camera.is_some() && o.mode != Mode::Live {
        return Err("--camera works only in the live window".into());
    }
    if o.mode != Mode::Live && o.seconds.is_some_and(|s| s > 600) {
        return Err("saved and timeline runs allow at most --seconds 600".into());
    }
    Ok(Some(o))
}

fn output_directory(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.split(['/', '\\']).any(|part| part == "..") {
        return Err("--out must not contain parent traversal".into());
    }
    let path =
        fs::canonicalize(value).map_err(|e| format!("cannot resolve --out directory: {e}"))?;
    if !path.is_dir() {
        return Err("--out must be a directory".into());
    }
    Ok(path)
}

fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| format!("cannot create {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[derive(Clone, Copy)]
struct Pose {
    visible: bool,
    x: f64,
    scale: f64,
    turn: f64,
}

fn ease(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Absent,
    Enter,
    Move,
    Pause,
    Leave,
}

struct Segment {
    kind: Kind,
    start: u64,
    duration: u64,
    periods: u64,
    amplitude: u64,
}

/// Same seeded visit plan as `sampleSchedule` in web/src/sample.ts:
/// mulberry32 over `seed ^ 0x5eed5eed`, integer milliseconds throughout.
fn schedule(seed: u32) -> (Vec<Segment>, u64) {
    let mut a = seed ^ 0x5eed_5eed;
    let mut next = move || {
        a = a.wrapping_add(0x6D2B_79F5);
        let mut t = a;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        t ^ (t >> 14)
    };
    let mut segments = Vec::new();
    let mut at = 0u64;
    let mut between = |lo: u64, hi: u64| lo + u64::from(next()) % (hi - lo + 1);
    let mut plan: Vec<(Kind, u64, u64, u64)> = Vec::new();
    for _ in 0..4 {
        plan.push((Kind::Absent, between(4_000, 15_000), 0, 0));
        plan.push((Kind::Enter, 3_000, 0, 0));
        let body = between(30_000, 60_000) as i64;
        let mut spent = 0i64;
        while spent < body {
            let moving = between(3_000, 6_000);
            let periods = 1 + between(0, 1);
            let amplitude = between(40, 120);
            let pause = between(1_000, 3_000);
            plan.push((Kind::Move, moving, periods, amplitude));
            plan.push((Kind::Pause, pause, 0, 0));
            spent += (moving + pause) as i64;
        }
        plan.push((Kind::Leave, 3_000, 0, 0));
    }
    for (kind, duration, periods, amplitude) in plan {
        segments.push(Segment {
            kind,
            start: at,
            duration,
            periods,
            amplitude,
        });
        at += duration;
    }
    (segments, at)
}

/// The visitor's pose. Without a seed it is the 28 second loop; with one,
/// the long seeded schedule. Reduced motion is the static pose.
fn pose(elapsed_ms: u64, reduced: bool, seed: Option<u32>) -> Pose {
    if let (Some(seed), false) = (seed, reduced) {
        return seeded_pose(elapsed_ms, seed);
    }
    legacy_pose(elapsed_ms, reduced)
}

fn seeded_pose(elapsed_ms: u64, seed: u32) -> Pose {
    let (segments, length) = schedule(seed);
    let t = elapsed_ms % length;
    let segment = segments
        .iter()
        .find(|s| t < s.start + s.duration)
        .unwrap_or(&segments[segments.len() - 1]);
    let progress = (t - segment.start) as f64 / segment.duration as f64;
    let at = |x: f64, scale: f64, turn: f64| Pose {
        visible: true,
        x,
        scale,
        turn,
    };
    match segment.kind {
        Kind::Enter => {
            let p = ease(progress);
            at(-0.2 + p * 0.72, 0.72 + p * 0.28, -0.06)
        }
        Kind::Move => {
            let phase = progress * PI * 2.0 * segment.periods as f64;
            at(
                0.52 + phase.sin() * (segment.amplitude as f64 / 1000.0),
                1.0,
                phase.sin() * 0.1,
            )
        }
        Kind::Pause => at(0.52, 1.0, 0.0),
        Kind::Leave => at(0.52 + ease(progress) * 0.78, 1.0, 0.08),
        Kind::Absent => Pose {
            visible: false,
            x: 1.3,
            scale: 1.0,
            turn: 0.08,
        },
    }
}

/// Same enter, move, hold and leave cycle as the page's unseeded sample.
fn legacy_pose(elapsed_ms: u64, reduced: bool) -> Pose {
    let t = if reduced {
        8_000.0
    } else {
        elapsed_ms as f64 % CYCLE_MS
    };
    let still = |x: f64, turn: f64| Pose {
        visible: true,
        x,
        scale: 1.0,
        turn,
    };
    if t < 3_000.0 {
        let p = ease(t / 3_000.0);
        Pose {
            visible: true,
            x: -0.2 + p * 0.72,
            scale: 0.72 + p * 0.28,
            turn: -0.06,
        }
    } else if t < 7_000.0 {
        let phase = (t - 3_000.0) / 4_000.0 * PI * 2.0;
        still(0.52 + phase.sin() * 0.04, phase.sin() * 0.1)
    } else if t < 11_000.0 {
        still(0.52, 0.0)
    } else if t < 14_000.0 {
        still(0.52 + ease((t - 11_000.0) / 3_000.0) * 0.78, 0.08)
    } else {
        Pose {
            visible: false,
            x: 1.3,
            scale: 1.0,
            turn: 0.08,
        }
    }
}

/// The head ellipse is rotated in local space and then scaled by the body
/// transform (x by `0.28 * scale`, y by `0.38 * scale`), as on the page's
/// canvas. Returns the frame-space center and those two scale factors.
fn head(p: Pose) -> (f64, f64, f64, f64) {
    let (sx, sy) = (0.28 * p.scale, 0.38 * p.scale);
    (p.x, 0.55 - 0.69 * sy, sx, sy)
}

const HEAD_RX: f64 = 0.25;
const HEAD_RY: f64 = 0.33;

/// Integer axis-aligned head bounds, or `None` when absent or clipped.
fn head_bounds(width: u32, height: u32, p: Pose) -> Option<(i32, i32, u32, u32)> {
    if !p.visible {
        return None;
    }
    let (cx, cy, sx, sy) = head(p);
    let (cos, sin) = (p.turn.cos(), p.turn.sin());
    let half_x = sx * (HEAD_RX * cos).hypot(HEAD_RY * sin);
    let half_y = sy * (HEAD_RX * sin).hypot(HEAD_RY * cos);
    let (w, h) = (f64::from(width), f64::from(height));
    let eps = 1e-9;
    let left = ((cx - half_x) * w + eps).floor();
    let right = ((cx + half_x) * w - eps).ceil();
    let top = ((cy - half_y) * h + eps).floor();
    let bottom = ((cy + half_y) * h - eps).ceil();
    if left < 0.0 || top < 0.0 || right > w || bottom > h || right <= left || bottom <= top {
        return None;
    }
    Some((
        left as i32,
        top as i32,
        (right - left) as u32,
        (bottom - top) as u32,
    ))
}

fn scene(width: u32, height: u32, p: Pose) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![255; w * h * 4];
    let (cx, cy, sx, sy) = head(p);
    let (cos, sin) = (p.turn.cos(), p.turn.sin());
    let body_half = 0.48 * 0.28 * p.scale;
    let body_top = 0.55 + 0.38 * p.scale * -0.2;
    for y in 0..h {
        let fy = (y as f64 + 0.5) / h as f64;
        for x in 0..w {
            let fx = (x as f64 + 0.5) / w as f64;
            // Room gradient with a doorway outline.
            let shade = 13.0 + fy * 28.0;
            let mut px = [shade * 0.6, shade * 0.9, shade * 1.3];
            if (0.22..0.78).contains(&fx)
                && fy < 0.84
                && (fx - 0.22).abs().min((fx - 0.78).abs()) < 0.006
            {
                px = [102.0, 112.0, 120.0];
            }
            if p.visible {
                if (fx - p.x).abs() <= body_half && fy >= body_top {
                    px = [89.0, 104.0, 119.0];
                }
                let (dx, dy) = ((fx - cx) / sx, (fy - cy) / sy);
                let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                if (u / HEAD_RX).powi(2) + (v / HEAD_RY).powi(2) <= 1.0 {
                    px = [184.0, 183.0, 176.0];
                    let eye = |sign: f64| {
                        ((u - sign * 0.09) / 0.055).powi(2) + ((v + 0.03) / 0.02).powi(2) <= 1.0
                    };
                    if eye(-1.0) || eye(1.0) {
                        px = [21.0, 29.0, 39.0];
                    }
                }
            }
            let at = (y * w + x) * 4;
            for c in 0..3 {
                out[at + c] = px[c].clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend_from_slice(data);
    out.extend_from_slice(&body);
    out.extend_from_slice(&crc32(&body).to_be_bytes());
}

struct Bits {
    out: Vec<u8>,
    acc: u32,
    count: u32,
}

impl Bits {
    /// Appends `n` bits of `value`, least significant bit first.
    fn put(&mut self, value: u32, n: u32) {
        self.acc |= value << self.count;
        self.count += n;
        while self.count >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.count -= 8;
        }
    }

    /// Appends a Huffman code, most significant bit first.
    fn code(&mut self, code: u32, n: u32) {
        self.put(code.reverse_bits() >> (32 - n), n);
    }

    fn literal(&mut self, byte: u8) {
        match byte {
            0..=143 => self.code(0x30 + u32::from(byte), 8),
            _ => self.code(0x190 + u32::from(byte) - 144, 9),
        }
    }

    /// A match of `length` (3..=258) at distance 4: one pixel back.
    fn repeat_pixel(&mut self, length: usize) {
        const BASE: [usize; 29] = [
            3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
            115, 131, 163, 195, 227, 258,
        ];
        const EXTRA: [u32; 29] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
        ];
        let index = BASE.iter().rposition(|&b| b <= length).unwrap_or(0);
        let symbol = 257 + index as u32;
        if symbol < 280 {
            self.code(symbol - 256, 7);
        } else {
            self.code(0xC0 + symbol - 280, 8);
        }
        self.put((length - BASE[index]) as u32, EXTRA[index]);
        self.code(3, 5); // distance code 3 is distance 4
    }
}

/// Deflate with one fixed-Huffman block. Runs of identical pixels become
/// matches one pixel back, which shrinks flat areas without any dependency.
fn deflate(raw: &[u8]) -> Vec<u8> {
    let mut bits = Bits {
        out: Vec::new(),
        acc: 0,
        count: 0,
    };
    bits.put(1, 1);
    bits.put(1, 2);
    let mut i = 0;
    while i < raw.len() {
        let mut run = 0;
        if i >= 4 {
            while i + run < raw.len() && run < 258 && raw[i + run] == raw[i + run - 4] {
                run += 1;
            }
        }
        if run >= 3 {
            bits.repeat_pixel(run);
            i += run;
        } else {
            bits.literal(raw[i]);
            i += 1;
        }
    }
    bits.code(0, 7); // end of block
    if bits.count > 0 {
        bits.out.push(bits.acc as u8);
    }
    bits.out
}

/// Minimal PNG: RGBA8, no filter, fixed-Huffman deflate.
fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in rgba.chunks(stride) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = vec![0x78, 0x01];
    z.extend_from_slice(&deflate(&raw));
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + u32::from(byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

struct Row {
    time: u64,
    state: &'static str,
    effect: f64,
    skull: u8,
    head: Option<(i32, i32, u32, u32)>,
    file: Option<String>,
}

const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>SpecterMirror preview</title>
<style>
body{margin:0;background:#080D14;color:#E5DFD1;font:15px system-ui,sans-serif;padding:16px}
h1{font-size:18px;margin:0 0 4px}p{color:#8D99A6;margin:4px 0 12px;max-width:70ch}
#view{width:min(100%,640px);image-rendering:pixelated;border:1px solid #151D27;display:block;background:#000}
.bar{display:flex;gap:10px;align-items:center;margin:10px 0;flex-wrap:wrap;max-width:640px}
button,select{background:#151D27;color:#E5DFD1;border:1px solid #8D99A6;padding:6px 12px;font:inherit}
button:focus-visible,select:focus-visible,input:focus-visible{outline:2px solid #72E7D6}
input[type=range]{flex:1;min-width:200px;accent-color:#72E7D6}
#info{font-variant-numeric:tabular-nums;color:#72E7D6;min-height:1.4em}
#grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(168px,1fr));gap:8px;margin-top:20px}
#grid button{padding:4px;text-align:left;font-size:12px;line-height:1.35}
#grid img{width:100%;display:block;image-rendering:pixelated}
#grid .skull{border-color:#8BCF83}
</style></head><body>
<h1>SpecterMirror offline preview</h1>
<p>__SUMMARY__ Generated visitor and the real engine; not webcam footage. A green border marks frames with a skull.</p>
<img id="view" alt="Rendered mirror frame">
<div class="bar"><button id="play">Pause</button>
<label>Speed <select id="speed"><option value="0.5">0.5x</option><option value="1" selected>1x</option><option value="2">2x</option><option value="4">4x</option></select></label>
<input id="seek" type="range" min="0" value="0" aria-label="Frame"></div>
<div id="info" role="status"></div>
<div id="grid"></div>
<script>
const frames=__FRAMES__, every=__EVERY__;
const view=document.getElementById('view'),seek=document.getElementById('seek'),info=document.getElementById('info'),play=document.getElementById('play'),speed=document.getElementById('speed'),grid=document.getElementById('grid');
seek.max=frames.length-1;let at=0,timer=null;
function show(i){at=Math.max(0,Math.min(frames.length-1,i));const f=frames[at];view.src=f.file;seek.value=at;
info.textContent=`${(f.t/1000).toFixed(1)} s | ${f.state} | effect ${f.effect.toFixed(0)}% | skull opacity ${f.skull}`+(f.head?` | head box ${f.head.join(', ')}`:' | no head in view');}
function tick(){if(at>=frames.length-1){stop();return;}show(at+1);timer=setTimeout(tick,every/Number(speed.value));}
function stop(){clearTimeout(timer);timer=null;play.textContent='Play';}
function start(){if(at>=frames.length-1)show(0);play.textContent='Pause';timer=setTimeout(tick,every/Number(speed.value));}
play.onclick=()=>timer?stop():start();seek.oninput=()=>{stop();show(Number(seek.value));};
speed.onchange=()=>{if(timer){clearTimeout(timer);timer=setTimeout(tick,every/Number(speed.value));}};
addEventListener('keydown',e=>{if(e.key==='ArrowRight'){stop();show(at+1);}if(e.key==='ArrowLeft'){stop();show(at-1);}});
frames.forEach((f,i)=>{const b=document.createElement('button');if(f.skull>0)b.className='skull';
const img=document.createElement('img');img.src=f.file;img.alt='';img.loading='lazy';b.append(img,`${(f.t/1000).toFixed(1)} s ${f.state}, skull ${f.skull}`);
b.onclick=()=>{stop();show(i);scrollTo(0,0);};grid.append(b);});
show(0);start();
</script></body></html>
"#;

fn write_page(o: &Options, rows: &[Row]) -> Result<PathBuf, String> {
    let shown: Vec<&Row> = rows.iter().filter(|r| r.file.is_some()).collect();
    let items: Vec<String> = shown
        .iter()
        .map(|r| {
            format!(
                "{{\"file\":\"{}\",\"t\":{},\"state\":\"{}\",\"effect\":{:.1},\"skull\":{},\"head\":{}}}",
                r.file.as_deref().unwrap_or(""),
                r.time,
                r.state,
                r.effect,
                r.skull,
                r.head
                    .map_or("null".into(), |(x, y, w, h)| format!("[{x},{y},{w},{h}]"))
            )
        })
        .collect();
    let summary = format!(
        "Seed {}, {}x{}, skull {}, reduced motion {}, {} frames.",
        o.seed,
        o.width,
        o.height,
        if o.skull { "on" } else { "off" },
        if o.reduced { "on" } else { "off" },
        shown.len()
    );
    let html = PAGE
        .replace("__SUMMARY__", &summary)
        .replace("__FRAMES__", &format!("[{}]", items.join(",")))
        .replace("__EVERY__", &o.every.to_string());
    let path = o.out.join("index.html");
    write_new(&path, html.as_bytes())?;
    Ok(path)
}

/// Best effort: ask the desktop to open the page. Failure only prints a hint.
fn open_viewer(path: &std::path::Path) {
    let opened = std::process::Command::new("xdg-open")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok();
    if !opened {
        println!("Could not run xdg-open; open the file above in a browser.");
    }
}

struct Sim<'a> {
    o: &'a Options,
    engine: MirrorEngine,
    skull: bool,
}

struct Step {
    output: Vec<u8>,
    state: &'static str,
    effect: f64,
    skull: u8,
    head: Option<(i32, i32, u32, u32)>,
}

const STATES: [&str; 5] = ["idle", "motion", "transforming", "haunted", "recovering"];

impl<'a> Sim<'a> {
    fn new(o: &'a Options) -> Result<Self, String> {
        let mut engine = MirrorEngine::new(o.width, o.height)?;
        engine.configure_automatic(true, o.reduced)?;
        engine.configure_seed(o.seed)?;
        engine.configure_skull(o.skull)?;
        Ok(Self {
            o,
            engine,
            skull: o.skull,
        })
    }

    fn toggle_skull(&mut self) -> Result<(), String> {
        self.skull = !self.skull;
        self.engine.configure_skull(self.skull)
    }

    /// Processes a webcam frame. `detection` is a finished face detection
    /// (from an earlier frame); none keeps the previous observation, which
    /// the engine expires after 500 ms. A detection without a face clears it.
    #[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
    fn step_camera(
        &mut self,
        frame: &[u8],
        time: u64,
        detection: Option<Detection>,
    ) -> Result<Step, String> {
        if let Some(found) = detection.filter(|_| self.skull) {
            match found.bounds {
                Some((x, y, w, h)) => {
                    self.engine
                        .observe_head(x, y, w, h, found.confidence, found.frame_ms as f64)?
                }
                None => self.engine.clear_head(),
            }
        }
        let output = self.engine.process_automatic_rgba(frame, time as f64)?;
        Ok(Step {
            output,
            state: STATES[usize::from(self.engine.haunt_state())],
            effect: f64::from(self.engine.haunt_strength()) / 60.0,
            skull: self.engine.skull_opacity(),
            head: detection.and_then(|d| d.bounds),
        })
    }

    /// Renders one frame at `time` ms; times must not decrease.
    fn step(&mut self, time: u64) -> Result<Step, String> {
        let o = self.o;
        let p = pose(
            time,
            o.reduced,
            (!o.fixed_visitor).then_some(o.seed ^ 0xa5a5_a5a5),
        );
        let head = head_bounds(o.width, o.height, p);
        if self.skull {
            match head {
                Some((x, y, w, h)) => self.engine.observe_head(x, y, w, h, 1000, time as f64)?,
                None => self.engine.clear_head(),
            }
        }
        let frame = scene(o.width, o.height, p);
        let output = self.engine.process_automatic_rgba(&frame, time as f64)?;
        Ok(Step {
            output,
            state: STATES[usize::from(self.engine.haunt_state())],
            effect: f64::from(self.engine.haunt_strength()) / 60.0,
            skull: self.engine.skull_opacity(),
            head,
        })
    }
}

fn describe(o: &Options) -> String {
    format!(
        "seed {} | {}x{} | skull {} | reduced motion {}",
        o.seed,
        o.width,
        o.height,
        if o.skull { "on" } else { "off" },
        if o.reduced { "on" } else { "off" }
    )
}

/// Webcam frames from an `ffmpeg` child process as raw RGBA at the
/// processing size. A reader thread keeps only the newest frame, so stale
/// frames are dropped instead of queued. The child is killed on drop.
#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
struct Camera {
    child: std::process::Child,
    latest: std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>,
    failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
impl Camera {
    fn open(device: &str, width: u32, height: u32) -> Result<Self, String> {
        use std::io::Read;
        use std::process::{Command, Stdio};
        use std::sync::{atomic::AtomicBool, Arc, Mutex};
        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "v4l2",
                "-i",
                device,
            ])
            .args([
                "-vf",
                &format!("crop='min(iw,ih*4/3)':'min(ih,iw*3/4)',scale={width}:{height}"),
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("cannot start ffmpeg (is it installed?): {e}"))?;
        let mut stdout = child.stdout.take().ok_or("ffmpeg has no output")?;
        let latest = Arc::new(Mutex::new(None));
        let failed = Arc::new(AtomicBool::new(false));
        let (slot, flag) = (Arc::clone(&latest), Arc::clone(&failed));
        let bytes = width as usize * height as usize * 4;
        std::thread::spawn(move || {
            let mut frame = vec![0; bytes];
            while stdout.read_exact(&mut frame).is_ok() {
                if let Ok(mut newest) = slot.lock() {
                    *newest = Some(frame.clone());
                }
            }
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        Ok(Self {
            child,
            latest,
            failed,
        })
    }

    fn take(&self) -> Result<Option<Vec<u8>>, String> {
        if self.failed.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("the camera stopped (ffmpeg ended; see its message above)".into());
        }
        Ok(self.latest.lock().ok().and_then(|mut f| f.take()))
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
impl Drop for Camera {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Detected head for the engine: bounds in unmirrored processing pixels,
/// confidence 800..=1000 and the time of the frame it was found in.
#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
#[derive(Clone, Copy)]
struct Detection {
    bounds: Option<(i32, i32, u32, u32)>,
    face: Option<(i32, i32, u32, u32)>,
    confidence: u16,
    frame_ms: u64,
}

/// SeetaFace frontal detector (via rustface) on its own thread. It receives
/// at most one pending frame and always works on the newest, so a slow
/// detection drops frames instead of delaying the mirror. Results carry the
/// frame time, so the engine's 500 ms age limit applies to them. The model is
/// embedded; nothing is read from or sent to the network.
#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
struct FaceFinder {
    pending: PendingFrame,
    results: std::sync::Arc<std::sync::Mutex<Option<Detection>>>,
    stopping: std::sync::Arc<std::sync::atomic::AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

/// One RGBA frame and its time, waiting for the detector thread.
#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
type PendingFrame = std::sync::Arc<(std::sync::Mutex<Option<(Vec<u8>, u64)>>, std::sync::Condvar)>;

#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
const FACE_MODEL: &[u8] = include_bytes!("../../../assets/models/seeta_fd_frontal_v1.0.bin");

/// Converts a detector face box into a skull box: a little wider, extended
/// upward over the forehead and crown, and clamped inside the frame.
fn head_from_face(
    face: (i32, i32, u32, u32),
    width: u32,
    height: u32,
) -> Option<(i32, i32, u32, u32)> {
    let (x, y, w, h) = (
        i64::from(face.0),
        i64::from(face.1),
        i64::from(face.2),
        i64::from(face.3),
    );
    let left = (x - w / 10).max(0);
    let right = (x + w + w / 10).min(i64::from(width));
    let top = (y - h * 3 / 10).max(0);
    let bottom = (y + h + h / 20).min(i64::from(height));
    (right > left && bottom > top).then(|| {
        (
            left as i32,
            top as i32,
            (right - left) as u32,
            (bottom - top) as u32,
        )
    })
}

#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
impl FaceFinder {
    fn start(width: u32, height: u32) -> Result<Self, String> {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Condvar, Mutex,
        };
        let model = rustface::read_model(FACE_MODEL)
            .map_err(|e| format!("cannot read the face model: {e}"))?;
        let pending: PendingFrame = Arc::new((Mutex::new(None), Condvar::new()));
        let results = Arc::new(Mutex::new(None));
        let finished = Arc::clone(&results);
        let stopping = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&stopping);
        let job = Arc::clone(&pending);
        let worker = std::thread::Builder::new()
            .name("preview-face".into())
            .spawn(move || {
                // Detect at half resolution from 320 x 240, then scale boxes back up.
                let factor = if width >= 320 { 2 } else { 1 };
                let (dw, dh) = (width / factor, height / factor);
                let mut detector = rustface::create_detector_with_model(model);
                detector.set_min_face_size(30);
                detector.set_score_thresh(2.0);
                detector.set_pyramid_scale_factor(0.8);
                detector.set_slide_window_step(4, 4);
                let mut gray = vec![0u8; width as usize * height as usize];
                let mut small = vec![0u8; dw as usize * dh as usize];
                loop {
                    let (rgba, frame_ms) = {
                        let (lock, ready) = &*job;
                        let Ok(mut slot) = lock.lock() else { return };
                        loop {
                            if stop.load(Ordering::Relaxed) {
                                return;
                            }
                            if let Some(work) = slot.take() {
                                break work;
                            }
                            let Ok(next) = ready.wait(slot) else { return };
                            slot = next;
                        }
                    };
                    for (g, px) in gray.iter_mut().zip(rgba.as_chunks::<4>().0) {
                        *g = ((u32::from(px[0]) * 77
                            + u32::from(px[1]) * 150
                            + u32::from(px[2]) * 29)
                            >> 8) as u8;
                    }
                    let (w, f) = (width as usize, factor as usize);
                    for (i, out) in small.iter_mut().enumerate() {
                        let (sx, sy) = (i % dw as usize * f, i / dw as usize * f);
                        let mut sum = 0u32;
                        for dy in 0..f {
                            for dx in 0..f {
                                sum += u32::from(gray[(sy + dy) * w + sx + dx]);
                            }
                        }
                        *out = (sum / (f * f) as u32) as u8;
                    }
                    let image = rustface::ImageData::new(&small, dw, dh);
                    let best = detector
                        .detect(&image)
                        .into_iter()
                        .max_by_key(|f| u64::from(f.bbox().width()) * u64::from(f.bbox().height()));
                    let found = best.map(|f| {
                        let b = f.bbox();
                        let k = factor as i32;
                        let face = (
                            b.x() * k,
                            b.y() * k,
                            b.width() * factor,
                            b.height() * factor,
                        );
                        // Accepted detections (score >= 2) map to 800..=1000.
                        let confidence = (800.0 + (f.score() - 2.0).max(0.0) * 50.0).min(1000.0);
                        (face, confidence as u16)
                    });
                    let detection = Detection {
                        bounds: found.and_then(|(face, _)| head_from_face(face, width, height)),
                        face: found.map(|(face, _)| face),
                        confidence: found.map_or(0, |(_, c)| c),
                        frame_ms,
                    };
                    if let Ok(mut latest) = finished.lock() {
                        *latest = Some(detection);
                    }
                }
            })
            .map_err(|e| format!("cannot start face detector: {e}"))?;
        Ok(Self {
            pending,
            results,
            stopping,
            worker: Some(worker),
        })
    }

    /// Offers the newest frame; an older pending frame is replaced.
    fn offer(&self, rgba: &[u8], frame_ms: u64) {
        let (lock, ready) = &*self.pending;
        if let Ok(mut slot) = lock.lock() {
            *slot = Some((rgba.to_vec(), frame_ms));
            ready.notify_one();
        }
    }

    /// The most recent finished detection, if any arrived since last call.
    fn latest(&self) -> Option<Detection> {
        self.results
            .lock()
            .ok()
            .and_then(|mut latest| latest.take())
    }
}

#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
impl Drop for FaceFinder {
    fn drop(&mut self) {
        let (lock, ready) = &*self.pending;
        if let Ok(_slot) = lock.lock() {
            self.stopping
                .store(true, std::sync::atomic::Ordering::Relaxed);
            ready.notify_one();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// The live window is built only for development hosts.
#[cfg(any(target_arch = "x86", target_arch = "wasm32"))]
fn live(_: &Options) -> Result<(), String> {
    Err("the live window is not built for this target; use --save or --no-frames".into())
}

/// Shows the mirror in an X11 window at wall-clock speed times `--speed`.
#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
fn live(o: &Options) -> Result<(), String> {
    if std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty()) {
        return Err(
            "no DISPLAY is set, so a window cannot open. Use --save for PNG \
                    frames and a viewer page, or --no-frames for a timeline."
                .into(),
        );
    }
    let mut sim = Sim::new(o)?;
    let camera = match &o.camera {
        Some(device) => Some(Camera::open(device, o.width, o.height)?),
        None => None,
    };
    let faces = match camera {
        Some(_) => Some(FaceFinder::start(o.width, o.height)?),
        None => None,
    };
    let mut shown_face: Option<(i32, i32, u32, u32)> = None;
    let mut face_count = 0u64;
    let (w, h) = (o.width as usize, o.height as usize);
    let scale = match o.scale {
        1 => Scale::X1,
        4 => Scale::X4,
        _ => Scale::X2,
    };
    let mut window = Window::new(
        "SpecterMirror preview",
        w,
        h,
        WindowOptions {
            scale,
            ..WindowOptions::default()
        },
    )
    .map_err(|e| format!("cannot open a window: {e}"))?;
    window.set_target_fps(30);
    println!(
        "{} | {} | speed {}x. Space pauses, K toggles the skull, Escape or Q quits.",
        describe(o),
        o.camera
            .as_deref()
            .map_or("generated visitor".into(), |d| format!("camera {d}")),
        o.speed
    );
    if camera.is_some() {
        println!(
            "Camera frames stay on this computer. A local frontal-face detector places the \
             skull; it does not identify anyone. Look at the camera after the effect rises."
        );
    }
    let mut buffer = vec![0u32; w * h];
    let mut clock = 0.0f64;
    let mut last = Instant::now();
    let mut next_title = Instant::now();
    let mut paused = false;
    let mut label = String::new();
    let mut last_state = "";
    while window.is_open() && !window.is_key_down(Key::Escape) && !window.is_key_down(Key::Q) {
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f64() * 1000.0;
        last = now;
        if window.is_key_pressed(Key::Space, KeyRepeat::No) {
            paused = !paused;
        }
        if window.is_key_pressed(Key::K, KeyRepeat::No) {
            sim.toggle_skull()?;
            println!("skull {}", if sim.skull { "on" } else { "off" });
        }
        if !paused {
            clock += dt * o.speed as f64;
            let time = clock as u64;
            if o.seconds.is_some_and(|s| time >= s * 1000) {
                break;
            }
            let step = match (&camera, &faces) {
                (Some(camera), Some(faces)) => match camera.take()? {
                    Some(frame) => {
                        let found = faces.latest();
                        if let Some(d) = found {
                            if d.face.is_some() != shown_face.is_some() {
                                face_count += 1;
                                println!(
                                    "{:>6.1} s  {}",
                                    time as f64 / 1000.0,
                                    if d.face.is_some() {
                                        "face found"
                                    } else {
                                        "face lost"
                                    }
                                );
                            }
                            shown_face = d.face;
                        }
                        faces.offer(&frame, time);
                        Some(sim.step_camera(&frame, time, found)?)
                    }
                    None => None,
                },
                _ => Some(sim.step(time)?),
            };
            let Some(step) = step else {
                window
                    .update_with_buffer(&buffer, w, h)
                    .map_err(|e| format!("cannot draw: {e}"))?;
                continue;
            };
            for (dst, px) in buffer.iter_mut().zip(step.output.as_chunks::<4>().0) {
                *dst = (u32::from(px[0]) << 16) | (u32::from(px[1]) << 8) | u32::from(px[2]);
            }
            if let (true, Some((x, y, fw, fh))) = (o.show_face, shown_face) {
                // The output is mirrored, so mirror the outline once as well.
                let left = w as i64 - i64::from(x) - i64::from(fw);
                outline(&mut buffer, w, h, left, i64::from(y), fw, fh);
            }
            if step.state != last_state {
                println!("{:>6.1} s  {}", time as f64 / 1000.0, step.state);
                last_state = step.state;
            }
            label = format!(
                "{:.1} s | {} | effect {:.0}% | skull {}{}",
                time as f64 / 1000.0,
                step.state,
                step.effect,
                step.skull,
                if sim.skull { "" } else { " (off)" }
            );
        }
        if now >= next_title {
            next_title = now + Duration::from_millis(250);
            window.set_title(&format!(
                "SpecterMirror | {label}{}",
                if paused { " | paused" } else { "" }
            ));
        }
        window
            .update_with_buffer(&buffer, w, h)
            .map_err(|e| format!("cannot draw: {e}"))?;
    }
    if face_count > 0 || faces.is_some() {
        println!("face state changes: {face_count}");
    }
    Ok(())
}

/// Draws a one-pixel green rectangle, clipped to the buffer.
#[cfg(not(any(target_arch = "x86", target_arch = "wasm32")))]
fn outline(buffer: &mut [u32], w: usize, h: usize, x: i64, y: i64, bw: u32, bh: u32) {
    let (x1, y1) = (x + i64::from(bw) - 1, y + i64::from(bh) - 1);
    let mut put = |px: i64, py: i64| {
        if (0..w as i64).contains(&px) && (0..h as i64).contains(&py) {
            buffer[py as usize * w + px as usize] = 0x8B_CF83;
        }
    };
    for px in x..=x1 {
        put(px, y);
        put(px, y1);
    }
    for py in y..=y1 {
        put(x, py);
        put(x1, py);
    }
}

fn run(o: &Options) -> Result<(), String> {
    if o.mode == Mode::Live {
        return live(o);
    }
    let save = o.mode == Mode::Save;
    let mut sim = Sim::new(o)?;
    if save {
        fs::create_dir_all(&o.out)
            .map_err(|e| format!("cannot create {}: {e}", o.out.display()))?;
        if fs::read_dir(&o.out)
            .map_err(|e| format!("cannot inspect output directory: {e}"))?
            .next()
            .is_some()
        {
            return Err(
                "output directory must be new or empty; existing files were preserved".into(),
            );
        }
    }
    println!("{}", describe(o));
    println!("time_s  state         effect%  skull_opacity  head_box(unmirrored)");
    let mut rows = Vec::new();
    for time in (0..=o.seconds.unwrap_or(60) * 1000).step_by(100) {
        let step = sim.step(time)?;
        if time % o.every == 0 {
            println!(
                "{:>6.1}  {:<12}  {:>6.1}  {:>13}  {}",
                time as f64 / 1000.0,
                step.state,
                step.effect,
                step.skull,
                step.head.map_or("-".into(), |b| format!("{b:?}")),
            );
            let file = if save {
                let name = format!("frame_{time:06}.png");
                let path = o.out.join(&name);
                write_new(&path, &png(o.width, o.height, &step.output))?;
                Some(name)
            } else {
                None
            };
            rows.push(Row {
                time,
                state: step.state,
                effect: step.effect,
                skull: step.skull,
                head: step.head,
                file,
            });
        }
    }
    if save {
        let page = write_page(o, &rows)?;
        let absolute = fs::canonicalize(&page).unwrap_or(page);
        println!(
            "wrote {} PNG frames and a viewer.\nOpen: file://{}",
            rows.len(),
            absolute.display()
        );
        if o.open {
            open_viewer(&absolute);
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match parse() {
        Ok(None) => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Ok(Some(options)) => match run(&options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("preview: {message}");
                ExitCode::FAILURE
            }
        },
        Err(message) => {
            eprintln!("preview: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(all(test, not(any(target_arch = "x86", target_arch = "wasm32"))))]
mod tests {
    use super::*;

    #[test]
    fn output_validation_and_writes_preserve_existing_files() {
        let root = std::env::temp_dir().join(format!(
            "specter-preview-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let file = root.join("index.html");
        write_new(&file, b"original").unwrap();
        assert!(write_new(&file, b"replacement").is_err());
        assert_eq!(fs::read(&file).unwrap(), b"original");
        assert!(output_directory(root.join("../outside").to_str().unwrap()).is_err());
        assert!(output_directory("").is_err());
        assert_eq!(
            output_directory(root.to_str().unwrap()).unwrap(),
            fs::canonicalize(&root).unwrap()
        );
        fs::remove_file(file).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn face_to_head_bounds_are_expanded_and_clipped() {
        assert_eq!(
            head_from_face((100, 50, 50, 60), 320, 240),
            Some((95, 32, 60, 81))
        );
        assert_eq!(
            head_from_face((0, 0, 50, 60), 320, 240),
            Some((0, 0, 55, 63))
        );
        assert_eq!(head_from_face((-20, -20, 10, 10), 320, 240), None);
        assert_eq!(head_from_face((i32::MAX, 0, u32::MAX, 20), 320, 240), None);
    }

    #[test]
    fn idle_detector_stops_and_joins_without_a_frame() {
        let mut finder = FaceFinder::start(160, 120).unwrap();
        let worker = finder.worker.take().unwrap();
        drop(finder);
        worker.join().unwrap();
    }

    #[test]
    fn detector_results_keep_only_the_newest_observation() {
        use std::sync::{atomic::AtomicBool, Arc, Condvar, Mutex};
        let finder = FaceFinder {
            pending: Arc::new((Mutex::new(None), Condvar::new())),
            results: Arc::new(Mutex::new(None)),
            stopping: Arc::new(AtomicBool::new(false)),
            worker: None,
        };
        for frame_ms in [10, 20] {
            *finder.results.lock().unwrap() = Some(Detection {
                bounds: None,
                face: None,
                confidence: 0,
                frame_ms,
            });
        }
        assert_eq!(finder.latest().unwrap().frame_ms, 20);
        assert!(finder.latest().is_none());
    }
}
