# Kiro development

The first task in `01-core-engine` was executed in a Kiro session and reviewed:
RGB24 grayscale conversion, Sobel L1 edges and image-pipeline tests. Both
functions use caller-owned buffers and preserve output on invalid input.
The frame contract and existing mirror behavior are unchanged.

Verification: core tests, formatting, clippy, WASM and i686 compilation.
Review added mixed-gradient and short-output mutation checks. No hook execution
or physical hardware performance is recorded for this task.

The second task in `01-core-engine` was executed in a Kiro session and reviewed:
4 x 4 Bayer dithering, a golden-output fixture and buffer-reuse tests. Review
converted the fixture to one plain PGM image so standard readers can open it.
Core tests, formatting, clippy, WASM and i686 checks cover this task.

The first task in `03-web-demo` now has checked RGBA interfaces, bindings and
browser processing. Tests cover mode output, orientation, input errors,
camera lifecycle and source switching. Review added regression cases for
unsaturated edges and camera loss during engine initialization. Chromium
verification used the generated scene and simulated camera input, not a
physical webcam. Hook execution has not been verified for this task.

The timestamped-history task in `04-haunted-experience` has core and adapter
tests for delay/trail combinations, literal pixels, warm-up, wraparound, reset,
safe timestamps and error preservation. It supplies the timed WASM methods
used by the browser controls. Review adds fallible storage reservation and an
allocation-capacity error test. The session reported manual checks and no hook
execution; no physical hardware or performance measurement is established.

The motion/state task in `04-haunted-experience` adds strict changed-pixel
thresholds, normalized centroids, hysteresis and the five-state interaction.
Its tests cover literal timelines, validation order, reset, error preservation
and bounded generated sequences. The session reported manual commands and no
hook execution. Core APIs alone do not establish browser interaction, head
localization, physical webcam support or measured target performance.

The skull-layer task in `04-haunted-experience` provides
`render_skull_rgba` for caller-supplied bounds, with original golden masks at
160 x 120 and 320 x 240. Tests cover literal blends, clipped geometry,
extreme positions, invalid arguments and deterministic generated cases.
Review replaced a 64-bit-only test value with a portable oversized dimension.
The session reported manual checks and no hook execution. This renderer does
not locate heads or establish camera alignment or target performance.

The WASM skull task in `04-haunted-experience` connects supplied head bounds
to timed translucent events and the browser sample's known head geometry.
Tests cover mirrored placement, fades, freshness, confidence, scratch reuse,
reset and source changes. Review strengthened the sample-to-camera test to
require a valid sample observation before switching. Production WASM was
checked at both processing sizes, with browser sample and simulated-camera
cleanup checks. The session reported manual commands and no hook execution.
Camera head localization and physical alignment remain unverified.

The seeded-timing and desktop-preview extension adds deterministic timing
seeds, longer generated visits and a development-only local face detector.
Review added cadence-independent fractional accumulation, bounded detector
results, worker shutdown and argument-validation checks. The X11 preview,
PNG output and camera-child cleanup were exercised with generated frames;
the production WASM was checked for seed replay and reduced motion. These
checks do not establish physical-camera alignment or target performance.

The hook definitions target the installed Kiro IDE 0.11.107 surface:
`.kiro/hooks/*.kiro.hook`, `when.type: userTriggered` and
`then.type: runCommand` with a literal command. The installed runtime's
schema and command executor were inspected. Hooks are manual to avoid
repeated builds. Open the project in that version, inspect the definitions
and approve each command before running. Newer releases may use another
schema; check their documentation before migrating. Configuration inspection
is not a claim that a hook ran or that it consumed no credits.

Record genuine completed work here with: spec/task, Kiro version and surface,
changed files, observed hook commands/results, produced tests, architecture
changes and reviewed commit or PR. Keep raw sessions and account data out of
the source. Do not mark tasks complete without implementation evidence.

Browser automatic output now consumes the shared director's temporal requests
and renders one 3200 ms skull apparition per selected event. The progression
still controls palette strength. Session seeds and resets replay both policies;
skull toggles preserve temporal history and progression. Public WASM signatures
and browser controls are unchanged.

Integration verification includes independent expected pixels, replay/seed
variation, neutral opening, event cooldowns, invalid-input twins, cancellation,
reset behavior and production WASM at 160 x 120 and 320 x 240. The production
detector worker initializes and returns no face for a generated blank frame.
Generated-camera page checks use scripted head observations to isolate
placement and lifecycle behavior; they do not measure detector accuracy.
Physical webcam alignment and target-laptop resource acceptance remain open.
The session reported no hook execution.

The native kiosk now supports opt-in RGB24/YUYV V4L2 capture through pinned
v4l 0.14.0, with bounded newest-frame storage, sample fallback and joined
shutdown. Native tests cover format negotiation, padded rows, conversion,
mirroring, timeout recovery, sequence wrap, source resets and cleanup. Review
replaced private-index startup assumptions with finite public-API waits and
added explicit bindgen prerequisites to CI. A synthetic V4L2 interface tested
the actual backend with fewer granted buffers, repeated startup timeouts,
frame delivery, disconnect fallback and Escape exit. This is generated input,
not physical camera validation. The session reported no hook execution;
antiX execution, laptop performance and actual camera acceptance remain open.
