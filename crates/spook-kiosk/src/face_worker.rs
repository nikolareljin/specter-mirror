//! One nonblocking request to a killable local detector process. No camera access.
use crate::presence::Face;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};

const BYTES: usize = 160 * 120;
const MODEL: &[u8] = include_bytes!("../../../assets/models/seeta_fd_frontal_v1.0.bin");

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut detector = rustface::create_detector_with_model(rustface::read_model(MODEL)?);
    detector.set_min_face_size(30);
    detector.set_score_thresh(2.0);
    detector.set_pyramid_scale_factor(0.8);
    detector.set_slide_window_step(4, 4);
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    output.write_all(b"R")?;
    output.flush()?;
    let mut pixels = [0; BYTES];
    loop {
        match input.read_exact(&mut pixels) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        }
        let faces = detector.detect(&rustface::ImageData::new(&pixels, 160, 120));
        let face = faces
            .iter()
            .filter_map(|f| {
                let b = f.bbox();
                let (x, y) = (b.x().max(0) as u32, b.y().max(0) as u32);
                let (right, bottom) = (
                    (i64::from(b.x()) + i64::from(b.width())).clamp(0, 160) as u32,
                    (i64::from(b.y()) + i64::from(b.height())).clamp(0, 120) as u32,
                );
                let face = Face {
                    x,
                    y,
                    width: right.saturating_sub(x),
                    height: bottom.saturating_sub(y),
                    confidence: (800.0 + (f.score() - 2.0) * 50.0).min(1000.0) as u32,
                };
                (f.score().is_finite() && f.score() >= 2.0 && face.valid()).then_some(face)
            })
            .max_by_key(|f| {
                (
                    f.width * f.height,
                    std::cmp::Reverse(f.y),
                    std::cmp::Reverse(f.x),
                )
            });
        let values = face.map_or([0; 5], |f| [f.x, f.y, f.width, f.height, f.confidence]);
        for value in values {
            output.write_all(&value.to_be_bytes())?;
        }
        output.flush()?;
    }
}

enum State {
    Starting,
    Ready,
    Sending(usize),
    Receiving(usize),
}

pub struct Locator {
    child: Child,
    socket: UnixStream,
    state: State,
    deadline: u64,
    next_offer: u64,
    observed: u64,
    input: [u8; BYTES],
    reply: [u8; 20],
}

impl Locator {
    pub fn new(now: u64) -> io::Result<Self> {
        // Linux resolves this to the running executable, without a PATH lookup.
        Self::spawn(Command::new("/proc/self/exe").arg("--face-worker"), now)
    }

    fn spawn(command: &mut Command, now: u64) -> io::Result<Self> {
        let (socket, worker) = UnixStream::pair()?;
        socket.set_nonblocking(true)?;
        let child = command
            .stdin(Stdio::from(OwnedFd::from(worker.try_clone()?)))
            .stdout(Stdio::from(OwnedFd::from(worker)))
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            child,
            socket,
            state: State::Starting,
            deadline: now.saturating_add(8000),
            next_offer: now,
            observed: 0,
            input: [0; BYTES],
            reply: [0; 20],
        })
    }

    pub fn offer(&mut self, rgba: &[u8], width: usize, height: usize, at: u64, now: u64) {
        if !matches!(self.state, State::Ready)
            || now < self.next_offer
            || at > now
            || now - at > 500
            || ![(160, 120), (320, 240)].contains(&(width, height))
            || rgba.len() != width * height * 4
        {
            return;
        }
        for y in 0..120 {
            for x in 0..160 {
                let i = ((y * height / 120) * width + x * width / 160) * 4;
                self.input[y * 160 + x] = ((u32::from(rgba[i]) * 77
                    + u32::from(rgba[i + 1]) * 150
                    + u32::from(rgba[i + 2]) * 29
                    + 128)
                    >> 8) as u8;
            }
        }
        self.observed = at;
        self.next_offer = now.saturating_add(200);
        self.deadline = now.saturating_add(1500);
        self.state = State::Sending(0);
    }

    pub fn poll(&mut self, now: u64) -> io::Result<Option<(u64, Option<Face>)>> {
        if self.child.try_wait()?.is_some() {
            return Err(io::Error::other("face detector exited"));
        }
        if !matches!(self.state, State::Ready) && now > self.deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "face detector timed out",
            ));
        }
        let result = match self.state {
            State::Starting => {
                let mut ack = [0];
                match self.socket.read(&mut ack) {
                    Ok(1) if ack[0] == b'R' => {
                        self.state = State::Ready;
                        Ok(None)
                    }
                    Ok(_) => Err(io::Error::other("invalid face detector greeting")),
                    Err(e) => Err(e),
                }
            }
            State::Ready => Ok(None),
            State::Sending(offset) => match self.socket.write(&self.input[offset..]) {
                Ok(0) => Err(io::Error::other("closed detector input")),
                Ok(n) => {
                    self.state = if offset + n == BYTES {
                        State::Receiving(0)
                    } else {
                        State::Sending(offset + n)
                    };
                    Ok(None)
                }
                Err(e) => Err(e),
            },
            State::Receiving(offset) => match self.socket.read(&mut self.reply[offset..]) {
                Ok(0) => Err(io::Error::other("closed detector output")),
                Ok(n) => {
                    if offset + n < 20 {
                        self.state = State::Receiving(offset + n);
                        return Ok(None);
                    }
                    let values: [u32; 5] = std::array::from_fn(|i| {
                        let offset = i * 4;
                        u32::from_be_bytes([
                            self.reply[offset],
                            self.reply[offset + 1],
                            self.reply[offset + 2],
                            self.reply[offset + 3],
                        ])
                    });
                    let face = if values == [0; 5] {
                        None
                    } else {
                        let face = Face {
                            x: values[0],
                            y: values[1],
                            width: values[2],
                            height: values[3],
                            confidence: values[4],
                        };
                        if !face.valid() {
                            return Err(io::Error::other("invalid detector bounds"));
                        }
                        Some(face)
                    };
                    self.state = State::Ready;
                    Ok(Some((self.observed, face)))
                }
                Err(e) => Err(e),
            },
        };
        match result {
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(None)
            }
            other => other,
        }
    }
}

impl Drop for Locator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(script: &str) -> Locator {
        Locator::spawn(Command::new("python3").args(["-c", script]), 0).unwrap()
    }
    fn until(mut predicate: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !predicate() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    #[test]
    fn protocol_is_single_flight_keeps_capture_time_and_reaps_the_process() {
        let mut worker=fixture("import sys,struct,time\nsys.stdout.buffer.write(b'R');sys.stdout.buffer.flush()\ndata=sys.stdin.buffer.read(19200)\nassert len(data)==19200 and data[0]==77\nsys.stdout.buffer.write(struct.pack('>5I',10,20,30,40,800));sys.stdout.buffer.flush()\ntime.sleep(20)");
        until(|| {
            worker.poll(0).unwrap();
            matches!(worker.state, State::Ready)
        });
        let frame = [255, 0, 0, 255].repeat(160 * 120);
        worker.offer(&frame, 160, 120, 40, 50);
        worker.offer(&[0; 4], 1, 1, 49, 50);
        assert_eq!(worker.observed, 40);
        let mut observation = None;
        until(|| {
            observation = worker.poll(60).unwrap();
            observation.is_some()
        });
        assert_eq!(
            observation,
            Some((
                40,
                Some(Face {
                    x: 10,
                    y: 20,
                    width: 30,
                    height: 40,
                    confidence: 800
                })
            ))
        );
        worker.offer(&frame, 160, 120, 61, 61);
        assert!(matches!(worker.state, State::Ready));
        let pid = worker.child.id();
        drop(worker);
        assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    }
    #[test]
    fn startup_and_inference_timeouts_fail_and_drop_kills_silent_children() {
        for ready in [false, true] {
            let mut worker = fixture(if ready {
                "import sys,time\nsys.stdout.buffer.write(b'R');sys.stdout.buffer.flush()\ntime.sleep(20)"
            } else {
                "import time\ntime.sleep(20)"
            });
            if ready {
                until(|| {
                    worker.poll(0).unwrap();
                    matches!(worker.state, State::Ready)
                });
                worker.offer(&[0; 160 * 120 * 4], 160, 120, 0, 0);
            }
            assert_eq!(
                worker
                    .poll(if ready { 1501 } else { 8001 })
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::TimedOut
            );
            let pid = worker.child.id();
            drop(worker);
            assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        }
    }
    #[test]
    fn invalid_packet_eof_and_spawn_failure_are_errors() {
        assert!(Locator::spawn(&mut Command::new("/nonexistent/face-detector"), 0).is_err());
        let mut worker=fixture("import sys,time\nsys.stdout.buffer.write(b'R');sys.stdout.buffer.flush()\nsys.stdin.buffer.read(19200)\nsys.stdout.buffer.write(b'\\xff'*20);sys.stdout.buffer.flush()\ntime.sleep(20)");
        until(|| {
            worker.poll(0).unwrap();
            matches!(worker.state, State::Ready)
        });
        worker.offer(&[0; 160 * 120 * 4], 160, 120, 0, 0);
        until(|| worker.poll(10).is_err());
        drop(worker);
        let mut worker = fixture("pass");
        until(|| worker.poll(0).is_err());
    }
}
