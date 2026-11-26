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

Automatic haunting is enabled initially. The page draws a random 32-bit seed
for each started source; the shared Rust progression uses it to pick a clear
opening, a rise, a stillness grace and a return, each between 4 and 15 seconds.
The opening stays a clear mirror. Motion that establishes interaction then
raises temporal and spectral intensity along a smoothstep curve, so it starts
and ends gently. After the grace of stillness it eases back to a clear mirror.
A completed return draws fresh durations for the next visit. The same seed,
frames and times replay the same timing; `MirrorController.seed` holds it for
logging. Without a seed (no provider, or the core used directly) the original
fixed cycle applies: five seconds clear, six up, a ten-second grace and three
down, linearly. Starting a source draws a new seed. Preference changes restart the visit
with the same initial durations.

The generated visitor follows the same seed. With a seed it makes four visits
in a loop of roughly four minutes: a quiet gap of 4 to 15 seconds, an entrance,
30 to 69 seconds of movement blocks with 1 to 3 second pauses, then an exit. The
visitor's plan is derived from the seed, so one seed replays one sequence. Its
head box follows the same plan, and the skull appears during the long visits.
Without a seed, or with reduced motion on, the original 28-second loop and
static pose apply.

### Offline preview

`./dev preview` runs the same engine without a browser or camera. By default
it opens an X11 window and plays a generated visitor from the same plan live,
so you can watch the clear opening, the filter, the skull and the return. The
window title shows time, state, effect strength and skull opacity, and state
changes are printed. Space pauses, K toggles the skull, Escape or Q quits.
Pausing a camera freezes the display while its observation clock continues,
so a detection from before the pause expires normally on resume.
Saved output requires a new or empty directory. An explicit `--out` directory
must already exist; the default is created. Parent traversal is rejected.
Existing files are never replaced.

```bash
./dev preview --seed 3                    # live window, real time, 2x zoom
./dev preview --seed 3 --speed 4          # four times faster
./dev preview --seed 7 --scale 4 --no-skull
./dev preview --camera                    # your webcam, filters and skull on your face
./dev preview --camera /dev/video2 --show-face
./dev preview --seed 3 --save --open      # PNG frames and an index.html viewer
./dev preview --seed 3 --no-frames        # timeline only, no window or files
./dev preview --help
```

`--camera` reads the webcam through a local `ffmpeg` process (no network,
nothing saved), crops it to 4:3 and runs the same automatic filters, delay,
ghost trails and seeded timing. A local frontal-face detector (rustface with
the bundled SeetaFace model) runs on its own thread at half resolution, always
on the newest frame, and supplies the skull's head box: the face box widened
by 10% each side and extended 30% upward, clamped to the frame. Detections
with score 2 or more map to confidence 800 to 1000 and carry their frame's
time, so the engine's 500 ms age limit and immediate hiding apply. It finds a
face's position; it does not identify anyone. `--show-face` outlines the
detected face in green. Detection cost must be measured on the actual host.
Side profiles, dim light and fast movement can lose the
face, and the skull then disappears. `--speed` is rejected with
`--camera`, because a faster clock would make every detection too old. The
detector exists only in this preview: the web page and native application
still have no face localizer, so the page shows no skull on camera frames.
The window needs an X11 `DISPLAY`; without one, use `--save`, or
`xvfb-run -a ./dev preview --seconds 30` for a headless smoke test. The Rust
visitor approximates the page's drawing; its head boxes agree with
`sampleHeadBounds` for the same seed. The output is for inspection, not a
measurement. The window uses minifb, the same pinned crate as the native
application, as a development-only dependency of `spook-web`.

Renewed motion during recovery keeps the current intensity instead of jumping.
Motion measures changed pixels, not a face or a person's presence; uniform
lighting changes can also trigger it. Timing is seeded; event-type selection
remains a separate requirement. Head-aligned skulls are
described under Skull apparitions.

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

## Skull apparitions

The `Skull apparitions` option (checked by default, available with automatic
haunting) draws a translucent procedural skull over supplied head bounds. It
needs a head observation, and only the generated sample provides one: the sample
reports the known geometry of its own drawn head. A camera has no head
localizer yet, so camera frames never show a skull. The sample placement is
not face detection, and nothing here infers a head from motion.

Observations are unmirrored processing-frame pixels (`x`, `y`, `width`,
`height`) with confidence 0..1000 and the same finite integer millisecond clock
as the frame. The rectangle must be nonempty and entirely inside the frame, and
time must not go backwards until the engine is reset. An invalid observation is
rejected and changes nothing. Confidence below 800, an age over 500 ms, a future
stamp, or no bounds hides the skull at once and cancels its event. The mirror
transform runs once, on the final output: `output_x = frame_width - x - width`.

Event timeline, from the first eligible frame (automatic haunting, shared
strength 1500 or more, fresh confident bounds, reduced motion off):

| Time since start | Phase | Ramp |
| --- | --- | --- |
| 0 to 1200 ms | fade in | `t` |
| 1200 to 2000 ms | hold | 1200 |
| 2000 to 3200 ms | fade out | `3200 - t` |

Opacity is `(96 * ramp * min(strength, 6000) + 3,600,000) / 7,200,000`, so it
is at most 96, further scaled by strength / 6000, rounded half up. After an
event ends or is cancelled the next one waits at least 8000 ms (from the
nominal end, or from the frame that cancelled it). A large clock jump starts at
most one event and never replays missed ones. Stop, source changes and changed
automatic or reduced-motion preferences clear observations, cooldown and
opacity. Toggling the option clears skull state only; temporal history and
the clear-opening envelope continue. Reduced motion draws no skull, and manual
reflection settings are unchanged.

The engine holds one extra reusable RGBA frame (307,200 bytes at 320 x 240)
allocated when the option is configured, plus a few fields of metadata. This
is not a total memory measurement. The skull is drawn after the temporal and
palette result and never feeds motion or the history ring. No alignment with a
physical webcam, frame rate or target hardware has been measured.

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
