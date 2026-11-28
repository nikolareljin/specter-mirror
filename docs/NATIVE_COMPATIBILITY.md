# Native dependency investigation

The opt-in [idle artwork mode](IDLE_ARTWORK.md) adds offline museum images and
a local face detector process before the shared haunting engine. Its actual
target detector cost, accuracy and alignment remain unverified. The baseline
sample/camera validation report does not measure that mode.

| Component | Foundation choice | Remaining evidence |
| --- | --- | --- |
| Window | minifb 0.29.0, X11 only, MIT OR Apache-2.0 | i686 execution on antiX, old drivers and physical display |
| Capture | v4l 0.14.0, `v4l2` feature only, MIT | Provisional. Hardware, format negotiation, unplug and the i686 libc baseline on actual antiX |
| Core | No external dependencies | Physical timings and target instructions |

minifb supplies a software buffer without requiring an OpenGL or Vulkan
renderer. The X11 feature excludes its default Wayland integration. Its
native C shim still needs target-compatible libc headers during cross builds.
The Xvfb proof only establishes workstation rendering through a virtual X11
display, not webcam capture or physical target performance.

The [foundation build proof](https://github.com/nikolareljin/specter-mirror/actions/runs/43964852672/job/133641478473)
linked the native i686 executable and ran the host X11 proof under Xvfb.
It did not execute the i686 binary on antiX or verify a physical laptop.

## Capture backend choice

Both candidates were built from isolated manifests on the Ubuntu x86_64 host
for `x86_64-unknown-linux-gnu` and `i686-unknown-linux-gnu` (debug builds that
linked). Package counts are Linux normal and build dependencies, excluding the
root.

| | v4l 0.14.0 (`default-features = false`, `v4l2`) | nokhwa 0.10.11 (`default-features = false`, `input-v4l`) |
| --- | --- | --- |
| Packages | 35 | 58, including image, moxcms and pxfm |
| Licenses | v4l and v4l2-sys-mit MIT; transitive MIT, Apache-2.0, ISC, BSD-3-Clause, Unlicense, Unicode-3.0 | nokhwa Apache-2.0; same permissive families plus Zlib |
| Build tools | bindgen, libclang, `linux/videodev2.h` | the same, through its v4l dependency |
| Linked libraries | libgcc_s, libc, ld-linux | the same |
| Waiting | public `Handle::poll(events, timeout_ms)` and `Stream::set_timeout` | `Stream::next()` is called with no timeout, so a read blocks until a frame arrives |
| Metadata | `Format` stride and image size; per-buffer bytes used, sequence, timestamp | resolution, pixel format and data only |

nokhwa cannot be cancelled while a camera is silent and hides the row stride,
so v4l was selected. Neither candidate is shown to work on antiX or physical
cameras.

v4l 0.14.0 queues a buffer before waiting in `Stream::next()`. Retrying
`next()` after a timeout would queue that buffer twice. The kiosk lets the
first `next()` initialize the actual granted buffers, always sets a finite
100 ms timeout, and completes a pending read with public `dequeue()` calls
before using `next()` again. The recovery frame is discarded. Before a later `next()` call it waits for a ready frame, so repeated
timeouts do not discard every frame from a slow camera. The readiness poll
and stream read share a 100 ms total wait budget. No private initial index
or driver-buffer count is assumed.
Protocol tests cover timeouts, interrupted/nonblocking reads and recovery.
Corruption-flagged frames, oversized payloads and malformed timestamps are
rejected before copying.
The library may panic on some non-ENODEV teardown errors. Worker failures
trigger sample fallback; driver behavior still needs physical verification.

## Camera mode

`--camera /dev/videoN` is opt-in. The path must be absolute, `/dev/video`
followed by a canonical decimal index (no leading zeros, at most 65535). The
option cannot repeat and cannot be combined with `--self-test` or `--help`.
Arguments are checked before a window or device is opened. V4L2 opens the
device read/write for capture configuration. The kiosk never changes permissions, records nothing and uses no network.

Format selection uses only single-plane capture formats the driver lists.
RGB24 (`RGB3`) and YUYV are supported. The exact processing size (320 x 240 or
160 x 120) wins, with RGB24 preferred over YUYV. Otherwise the smallest 4:3
size at least as large as the processing size, up to 640 x 480, is used.
Stepwise ranges are tested at 160 x 120, 320 x 240 and 640 x 480. The driver's
answer is read back and must equal the request; stride, image size and width
(even for YUYV) are validated. MJPEG, multi-plane devices, non-4:3 sizes, tiny
sources and sources larger than 640 x 480 are rejected with a diagnostic and
the sample continues. There is no MJPEG decoder. A camera that offers only
MJPEG cannot be used.

Frames are converted on the display thread in integer arithmetic. YUYV uses
BT.601 limited range: with `c = Y - 16`, `d = U - 128`, `e = V - 128`,
`R = (298c + 409e + 128) >> 8`, `G = (298c - 100d - 208e + 128) >> 8`,
`B = (298c + 516d + 128) >> 8`, each clamped to 0..255. The source is
center-cropped to the largest 4:3 rectangle and each output pixel is the
rounded mean of its source block. A source of exactly the processing size is
copied. The result goes unmirrored into the shared temporal history, then is
packed with one horizontal reflection, then follows the existing fit,
delay, trails and Escape behavior. Reduced motion keeps camera frames live and
drops delay and trails; the sample still freezes. The window title is
unchanged.

Row padding is honored. A payload is accepted when it holds every row
(`(height - 1) * stride + row` bytes) and fits the buffer. Anything shorter
or longer is dropped before publishing and never displayed.

## Capture lifecycle and bounds

One worker thread opens the device, negotiates, streams and waits in slices of
at most 100 ms. Each complete frame is copied into a preallocated buffer,
stamped with the process monotonic clock at dequeue (not exposure time, and
not the driver clock, which is only checked for non-regression) and exchanged
with the display thread by swapping one of three equal buffers under a short
lock. A newer frame replaces an unread one, so there is no queue. The three
buffers hold at most 4 MiB each; 640 x 480 YUYV needs 3 x 614,400 bytes and
RGB24 at that size 3 x 921,600 bytes. The converter adds 307,200 bytes at
320 x 240 or 76,800 at 160 x 120. Four driver buffers are requested. Actual mapped lengths and the granted
count are controlled by the driver, and are outside the software totals above.
The 4 MiB validation bounds software buffers, not driver allocations.

The display thread never waits on the camera. A frame is used once, and only
if it is at most 500 ms old when taken. Sequence numbers advance modulo 2^32; nonzero driver timestamps must increase. A first frame must arrive within 2 s of start and
each later frame within 1 s of the previous one. Missing devices, permission
errors, unsupported formats, negotiation mismatch, disconnect, end of stream
and stalls each print one line and switch to the sample once. History is
cleared at that switch so no camera pixel reaches the sample. There is no
automatic reopen.

Escape, window close and every error path cancel the worker, wait for the
current device wait, stop streaming, join the thread and close the device.
Cancellation takes at most one wait slice plus the stop call. A driver call
that never returns would delay the final join. After a fallback the display
keeps running while the retired worker finishes.

Tests use a scripted fake device and a manual clock. They cover format
selection, negotiation mismatch, geometry limits, padding, conversion
vectors, crop and resize at both sizes, single reflection, newest-frame
replacement, age (500/501 ms), startup and stall limits, EOF, permission
failure, partial initialization, history reset, reduced motion and worker
cleanup. A synthetic V4L2 ABI fixture also exercised the actual backend with two
granted buffers, startup timeouts, frame delivery, disconnect fallback and
Escape cleanup. The original return rejected the valid two-buffer fixture;
the corrected backend passes. The fixture did not open a camera. The only
non-synthetic device paths checked were a missing node and `/dev/null`. No physical camera was opened, so the actual format list,
stride, timestamps, unplug behavior, timing and memory on the NB205 and D630
remain unverified.

## Display fit

The native window defaults to cover: crop the center of the low-resolution
processing frame to the window aspect, then use X11 software scaling to fill
the window. Crop edges round to source pixels. This does not increase the
processing resolution or mirror the image again. The appliance session requests
fullscreen; a normal development window remains resizable.

Run `cargo run --locked -p spook-kiosk -- --fit contain` to keep the entire
frame with letterboxing, or `--fit stretch` for nonuniform fill. Use `--help`
for examples, or `--self-test --fit cover` for a one-frame X11 proof.
`./demo native` uses the default cover setting.

## Animated diagnostic sample

The generated sample moves an asymmetric cyan bar across a dark gradient.
It runs through the shared timestamped history ring before one horizontal
reflection and display scaling. This is diagnostic input, not webcam footage
or a head-aligned effect. The window title stays stable for appliance startup.

```bash
cargo run --locked -p spook-kiosk -- --resolution 160x120 --delay-ms 400 --trails 128
cargo run --locked -p spook-kiosk -- --delay-ms 0 --trails 0
cargo run --locked -p spook-kiosk -- --reduced-motion --fit contain
```

Installed release binaries accept the same flags without Rust installed.
Default processing is 320 x 240, with delay 250 ms and trail weight 115/255.
Delay accepts 0..600 ms and trails accept 0..255. Reduced motion freezes the
generated sample and bypasses temporal blending. Camera input stays live;
see Camera mode above.

The sample preallocates source RGBA, processed RGBA and native pixels, plus
the shared bounded history. With the presentation buffer, these pixel arrays
total 3,686,400 bytes at 320 x 240 or 921,600 at 160 x 120. They are reused
while animating/resizing. These bounds exclude metadata, stack, libraries,
allocator overhead and X11 buffers; they are not a process RAM measurement.
Initialization reports reservation failures. The sample has no capture queue.

Unit tests check exact trails, delayed frames, reflection, stable buffer
pointers, rejected timestamps and static reduced motion. Actual laptop
camera operation, throughput and memory still require physical verification.

At 320 x 240 the reusable crop buffer holds 307,200 bytes, independent of
display dimensions. Input is limited to 640 x 480 and displays to 16,777,216
pixels; dimensions and lengths are validated before buffer mutation. The X11
library owns additional window buffers; the crop-buffer bound is not a total
process-memory measurement. Resize tests cover wide, tall, square and narrow
windows, rejected input and storage reuse. Physical display verification remains
separate from host tests and the virtual X11 proof.

## Opt-in measurements

`--metrics` prints one versioned JSON report to stdout after the worker is
joined, and `--run-for-ms N` (100..600000) ends a run through the normal
cancel, join and close path. Both are rejected with `--self-test`. Without
them nothing is measured and behavior is unchanged. The report keeps new camera
frames, sample updates, display submissions and idle display ticks apart.
Processing time (conversion plus temporal processing of one new source frame)
is separate from presentation and from display cadence, which is separate from
the configured 30 FPS. Summaries use fixed histograms (250 us bins to 100 ms
for durations, 1 ms bins to 500 ms for intervals) with exact count, maximum
and overflow count, so memory is constant for any run length. Capture
counters (published, overwritten unread, stale) survive fallback and
shutdown. Driver drops are not counted and are reported as `null`. A host
sample run under Xvfb exercised this on the development machine only.
[Native validation](NATIVE_VALIDATION.md) defines every field, the local runner
and the operator checklist. No measurement from an antiX guest, physical
laptop or webcam exists yet.
