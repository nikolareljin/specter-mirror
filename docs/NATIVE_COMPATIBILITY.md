# Native dependency investigation

| Component | Foundation choice | Remaining evidence |
| --- | --- | --- |
| Window | minifb 0.29.0, X11 only, MIT OR Apache-2.0 | i686 link/runtime, antiX drivers and physical display |
| Capture | None selected | Compare v4l and nokhwa on actual i686 antiX |
| Core | No external dependencies | Physical timings and target instructions |

minifb supplies a software buffer without requiring an OpenGL or Vulkan
renderer. The X11 feature excludes its default Wayland integration. Its
native C shim still needs target-compatible libc headers during cross builds.
The Xvfb proof only establishes workstation rendering through a virtual X11
display, not webcam capture or physical target performance.

Before choosing capture, record pinned versions, licenses, Linux backend
features, transitive dependencies, required native tools and target builds.
Test camera negotiation for RGB/YUYV/MJPEG as actually offered; verify
strides, missing devices, unplug, stale-frame dropping and clean shutdown.
Do not describe either candidate as i686-compatible before that evidence.
