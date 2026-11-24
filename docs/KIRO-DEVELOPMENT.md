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
