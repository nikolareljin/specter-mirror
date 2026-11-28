# Face-triggered native artwork

Build the optimized native executable, then choose a camera explicitly:

```bash
./dev build-i686
target/i686-unknown-linux-gnu/release/spook-kiosk --idle-artwork --camera /dev/video0
```

Use the actual device selected by [hardware discovery](HARDWARE.md), not an
assumed index. Without `--camera`, `--idle-artwork` is an offline slideshow
with no camera or detector process. The existing sample and manual camera
commands keep their behavior. Use a release build; unoptimized detection can
exceed its timeout. Escape and window close stop capture and the detector.

## Display behavior

- Three bundled public-domain paintings begin in their original colors: Island
  of the Dead, The Sleep of Reason Produces Monsters, and Still Life with a
  Skull and a Writing Quill. Each holds for 2 minutes of idle display time,
  then crossfades for 5 seconds. A seeded shuffle shows every image in each
  three-image group and avoids immediate repeats across group boundaries.
- Paintings keep their orientation and aspect ratio. This mode defaults to
  `--fit contain`; explicitly selecting cover can crop the composition.
- Fresh face observations sustained for more than 2000 ms start the live
  mirror, with an eased 1-second reveal from the visible photo to full-color
  webcam input. Exactly 2000 ms does not activate it.
- Each visit starts the shared seeded progression and director with a neutral
  opening. Delay and trails use one raw-camera history. Director-selected
  procedural skulls fade once for 3.2 seconds per event, with opacity <=96.
  Head placement uses frontal face bounds; physical alignment is unverified.
- After 3000 ms without a valid face, fade to a green version of the held photo
  over 1 second, then ease that same photo back to its original colors over
  6 seconds. Pause the shuffle throughout the live visit and recovery, then
  give the recovered photo a fresh 2-minute hold before the next shuffle.
  A returning visitor can interrupt recovery without jumping to another image.
  A brief missed detection does not end a visit. A still face can keep haunting
  active without requiring movement. Detection is not identity or gaze tracking.
- Camera loss or detector failure immediately shows artwork, clears the live
  state and reports its unavailable status. It does not prove the room is empty.
  There is no automatic camera reopen or detector restart in the same run.
- `--reduced-motion` shows the first painting without animation and switches
  modes without fades or green recovery. Live camera pixels remain live;
  haunting is suppressed.

The window title identifies artwork/live state and whether the camera remains
active. Camera capture continues locally during artwork when explicitly enabled.
The application sends no frames over the network and records no images.

## Bounds and lifecycle

Only unmirrored live camera frames reach the detector. Paintings and sample
frames never do. A separate copy of this executable loads the existing local
SeetaFace model and runs rustface at 160 x 120. One fixed 19200-byte grayscale
request is in flight, at most once per 200 ms, with a fixed 20-byte response.
The parent uses nonblocking local sockets. It allows 8 seconds to initialize
and 1.5 seconds per request; failure disables detection and kills/reaps the
child. Detection allocates internally in the child, outside the display loop.
The model and code licenses are in [dependencies](DEPENDENCIES.md) and
[asset provenance](../assets/PROVENANCE.md).

Observations retain capture time and expire after 500 ms. A completed negative,
low-confidence result or freshness gap resets the activation streak. Re-reading
one result cannot qualify a visit. Stop/new source and capture failure reset
state. Live images and bounds are mirrored once; paintings are not mirrored.
The raw-camera ring never contains artwork or a previous visit's history.

JPEGs are decoded once at startup, with a 4 MiB decode-buffer limit per image
and dimensions limited to 1600 x 1600. Cached textures, output and transition
buffers are reused. Three natural and three green textures total 1843200 bytes
at 320 x 240, or 460800 at 160 x 120. A reusable transition frame preserves the
visible image when a fade is interrupted. These figures are texture storage,
not process RAM measurements. Cubic integer easing softens fade endpoints.
No files or network downloads are needed at runtime.

`--run-for-ms N` still requests orderly timed exit. `--self-test`, `--metrics`,
`--delay-ms` and `--trails` are rejected with this mode: the director owns event
parameters, and the baseline metrics schema does not describe artwork/detector
work. Do not label a baseline validation report as evidence of this mode's cost.
Target CPU/RAM/frame times, real-face accuracy, eye alignment and antiX runtime
compatibility require separate measurements on each actual laptop. Host tests
and a linked i686 executable do not establish those results.
