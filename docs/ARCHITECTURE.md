# Architecture

`spook-core` owns portable frame validation and processing. `spook-kiosk`
owns Linux camera/window integration. `spook-web` exposes the engine to
the TypeScript frontend. The core cannot import window, camera or OS APIs.

Input -> validation -> normalization -> downscaling -> grayscale -> motion
analysis -> effects -> overlays -> palette -> display scaling -> output.
The current core implements RGB24 validation, grayscale conversion, Sobel edges,
Bayer dithering and mirrored packed RGB output. The WASM adapter provides three
manual modes and checked RGBA processing; motion and temporal effects remain
future work described in the [haunted interaction design](../.kiro/specs/04-haunted-experience/design.md).

`RgbFrame` borrows pixels. Width and height are nonzero. Stride is bytes per
row and must be at least `width * 3`. The last row need not include padding.
Arithmetic is checked before indexing. `copy_mirrored_rgb24` writes
`0x00RRGGBB` pixels into a caller-owned output buffer; trailing output stays
unchanged. Short output returns an error before writing.

The native spike pins minifb 0.29.0 with only its X11 feature. It displays a
synthetic gradient through a software buffer and does not capture a webcam.
V4L2 integration remains undecided pending the backend investigation in
[native compatibility](NATIVE_COMPATIBILITY.md).

No queues are needed for the synthetic source. Capture must later use a
bounded newest-frame policy. No backend or frame upload is required.
