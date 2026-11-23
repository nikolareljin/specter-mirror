# Native dependency investigation

| Component | Foundation choice | Remaining evidence |
| --- | --- | --- |
| Window | minifb 0.29.0, X11 only, MIT OR Apache-2.0 | i686 execution on antiX, old drivers and physical display |
| Capture | None selected | Compare v4l and nokhwa on actual i686 antiX |
| Core | No external dependencies | Physical timings and target instructions |

minifb supplies a software buffer without requiring an OpenGL or Vulkan
renderer. The X11 feature excludes its default Wayland integration. Its
native C shim still needs target-compatible libc headers during cross builds.
The Xvfb proof only establishes workstation rendering through a virtual X11
display, not webcam capture or physical target performance.

The [foundation build proof](https://github.com/nikolareljin/specter-mirror/actions/runs/43964852672/job/133641478473)
linked the native i686 executable and ran the host X11 proof under Xvfb.
It did not execute the i686 binary on antiX or verify a physical laptop.

Before choosing capture, record pinned versions, licenses, Linux backend
features, transitive dependencies, required native tools and target builds.
Test camera negotiation for RGB/YUYV/MJPEG as actually offered; verify
strides, missing devices, unplug, stale-frame dropping and clean shutdown.
Do not describe either candidate as i686-compatible before that evidence.

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

At 320 x 240 the reusable crop buffer holds 307,200 bytes, independent of
display dimensions. Input is limited to 640 x 480 and displays to 16,777,216
pixels; dimensions and lengths are validated before buffer mutation. The X11
library owns additional window buffers; the crop-buffer bound is not a total
process-memory measurement. Resize tests cover wide, tall, square and narrow
windows, rejected input and storage reuse. Physical display verification remains
separate from host tests and the virtual X11 proof.
