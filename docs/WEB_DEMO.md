# Web interface
The Vite/TypeScript page runs the shared Rust engine as WebAssembly. Frames
are processed in the browser and drawn to a Canvas 2D element. Nothing is
uploaded, recorded or sent to analytics.

## Build and run
Development tools: Rust 1.99.1 with the `wasm32-unknown-unknown` target,
`wasm-pack` (0.15.1 was used), Node.js 22.12+ and npm.
```bash
npm ci --prefix web
npm --prefix web run wasm:build   # wasm-pack, target web, output web/generated
npm --prefix web test             # Node built-in runner, injected fakes
npm --prefix web run build        # builds WASM, type-checks, then Vite
./demo web                        # npm run dev, builds WASM first
```
`wasm:build` uses `--no-opt`, so `wasm-opt` is not required. `web/generated/`
is build output and is not tracked. `predev` and `prebuild` regenerate it.
Rust tests for the engine run with `cargo test --locked -p spook-web`.

## Modes
All modes mirror the image horizontally once and return opaque RGBA.
- Mirror: the mirrored input. Intensity is ignored.
- Spectral Mirror: shared grayscale and Sobel; edges map to spectral cyan,
  with continuous edge strength from black to cyan, blended with the original
  by intensity.
- Cursed Portrait: shared grayscale and 4 x 4 Bayer dithering; lit pixels map
  to bone and dark pixels to midnight, blended with the original by intensity.
Blend: `(original * (255 - i) + processed * i + 127) / 255`.

## Dynamic reflection

Automatic haunting is enabled initially. The shared Rust progression holds a
clear reflection for five seconds from the first frame, then builds temporal
and spectral intensity over six seconds when motion has established interaction.
Stillness gets a ten-second grace period before a three-second fade to clear.
Renewed motion during recovery keeps the current intensity instead of jumping.
Motion measures changed pixels, not a face or a person's presence; uniform
lighting changes can also trigger it. This first automatic treatment is fixed;
seeded event selection and head-aligned geometry remain separate requirements.

Turn automatic haunting off for manual settings. Its preferences, source
changes and Stop reset interaction/history. Reduced motion keeps automatic
output clear while natural camera movement stays live. Hiding the page stops
the mirror and releases the camera; returning requires an explicit Start.

The automatic path compares raw grayscale frames at most every 100 ms, before
temporal layers or palette processing. It reuses one additional grayscale
history buffer (76,800 bytes at 320 x 240). Effect weights change without
clearing the shared frame ring, which stays warm during the clear opening.
The display shows the current phase and effective intensity. These readings
are not face detection or performance measurements.

Reflection delay (0..600 ms) and ghost trails (0..100%) work independently or
together with every palette. Restless Reflection, Ghost Trails, Cursed Glass
and Clear Reflection set these controls; Custom keeps your chosen values.
Temporal processing runs on raw RGBA before the selected palette and single
horizontal reflection. Reduced motion disables both temporal layers while
preserving the requested settings for later restoration.

History samples at most once per 100 ms into one bounded ring. At 320 x 240
the ring uses 2,457,600 pixel bytes; it never exceeds 4 MiB or eight frames.
The adapter also holds one reusable RGBA working frame and copies output at
the WASM boundary. These bounds are not a total browser RAM measurement.
Short or sparse history uses the oldest available sample, so the requested
delay is not guaranteed during warm-up. Changing temporal settings, stopping
or switching sources clears history. Invalid frames and timestamps are rejected.

## Sources
- Start camera requests video only after you press the button, preferring a
  front-facing device at 320 x 240. The image is center-cropped to 4:3.
- Use sample scene needs no permission. An original generated visitor enters,
  gestures, holds still and leaves on a 28-second loop, then passes through the
  same WASM path. It is not webcam footage or a recording. With reduced motion
  enabled it is static.
  The quiet interval lets automatic mode recover before the next visitor.
- Stop, switching sources, `pagehide` and camera track loss stop all tracks,
  cancel the frame loop and free the engine. A camera permission result that
  arrives after Stop or a source switch is stopped immediately.

Full screen fills the display with a centered cover crop while keeping the
processing size at 320 x 240. Press Escape or Exit full screen to return;
Stop mirror also releases the camera. Source changes, errors and pagehide
close the owned fullscreen view. If the browser lacks fullscreen support or
rejects the request, the mirror continues in the page. Fullscreen requests
require a button click and never request camera access by themselves.

The page reports capture, Rust/WASM and draw times separately, plus a
measured frame rate over recent frames. These are readings from the current
browser, not performance claims.

## Status
Physical webcam behavior and other browsers still require manual verification.
The camera requires a secure context (HTTPS or localhost).
GitHub Pages retains `/specter-mirror/` for direct About navigation and asset
URLs. The Pages workflow validates pull requests without uploading or deploying.
Only verified `main` builds publish through `ci-helpers@production`. Build jobs
have read permissions; the separate deployment job receives Pages permissions.
The mirror artwork is an original illustration, not a processed sample.

## Publication

Run `bash scripts/pages.sh build` to test and build. After reviewing the output,
run `bash scripts/pages.sh record` with `SPECTER_PAGES_HELPERS` pointing to the
pinned helper checkout. This local operation inspects the output and records
`web/pages-manifest.json`. It requires the operator's local publication setup.
Commit the manifest with each site change. CI reads the manifest from its exact
commit object, so build scripts cannot approve output by rewriting the working
copy. CI cannot record a replacement manifest.

Before upload, `bash scripts/pages.sh check` requires the complete output tree
to match those digests and reruns the publication tools. Unexpected files,
links, source maps, broken project asset URLs and missing tools stop publication.
Only `web/dist` is uploaded. Tool diagnostics stay outside Git and are never
included in logs or artifacts. Application builds need no publication tooling.

The publication tooling uses script-helpers revision
`548597c18f70c010a289c29305c45fe84863d12c`, Gitleaks 8.30.1 and Foxguard 0.14.1.
Tool setup is for the build runner; none is installed on a visitor's computer.
A successful deployment is required before the public demo URL is available.
