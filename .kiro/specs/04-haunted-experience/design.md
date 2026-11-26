# Composable haunting design

## Shared processing

Keep motion observations, interaction state, seeded event selection and visual
composition in spook-core. Native and WASM adapters supply frames, monotonic
elapsed time, a seed, preferences and measured quality inputs. Browser code
owns DOM controls, Canvas presentation and optional Web Audio, not a separate
interaction policy. Do not add an ML or remote inference dependency.

Input -> validation -> normalization/downscale -> motion observation ->
interaction state -> eligible event selection -> temporal layers -> distortion
-> apparition geometry -> palette -> frost/vignette -> presentation.

Mirror exactly once. Define normalized motion locations in source coordinates;
the presentation transform converts both pixels and event positions through
the same crop, scale and reflection. Motion observes changed pixels, not faces.
Stillness retains presence for a bounded grace period and then recovers; this
is a timed estimate rather than proof that a visitor departed.

## Independent layers

Each layer has an enable flag, bounded intensity and supported parameters.
Recipes are data: eligible layers, timing ranges, intensity ceilings and cost
limits. Provide Restless Reflection (delay/trails), Cursed Glass (frost/portrait),
Something Behind You (apparition/spectral), Poltergeist (localized distortion)
and a custom recipe. Haunt me selects among compatible events within a recipe;
manual modes remain available. These are target behaviors, not implemented modes.

Share one preallocated, timestamped history ring between delay and trails.
Retain at most eight low-resolution frames within 4 MiB; sample history at
bounded intervals rather than every display frame. Select the nearest usable
past sample and define warm-up behavior explicitly. Never make a ring per
effect or retain camera recordings. Clear the ring on resize/source changes.

Apparitions use original procedural geometry. Motion-positioned apparitions
are distinct from optional head-aligned skulls. Head alignment requires a
validated localizer observation (source bounds, confidence and timestamp),
not a motion centroid. Suppress stale or uncertain observations and transform
bounds through the same crop/reflection as pixels. Keep localization optional
and outside the frame hot loop; select its backend only after target testing.
The browser adapter runs rustface with the bundled SeetaFace model in a worker
with its own WASM instance. A tracker owns one in-flight request, a 200 ms
cadence, a generation and request id, timeouts and a single latest observation
with the original frame time. It widens the face to a head box with integer
arithmetic, scales it into the unmirrored processing frame and leaves
mirroring to the engine. `MirrorController` offers frames and reads the latest
fresh result through optional dependencies, so failures never become mirror
failures. This is a browser-only backend; native localization is unchanged.
No identity recognition is required. Frost and vignette are
cheap spatial treatments with bounded animation; distortion uses integer
sampling and clamps every source coordinate. Compose into caller-owned buffers.

## Interaction and event selection

Keep the five-state machine separate from rendering. Inject elapsed time and
motion observations so state transitions can be tested without sleeping.
Clamp or reject regressing time consistently before mutation. Large time jumps
must not require a loop for every skipped frame. Validate policy configuration
once; invalid frame input must not advance timers or modify output/history.

Automatic mode starts with zero effect weights and a neutral live introduction.
The state machine supplies phase and elapsed time; a separate bounded envelope
ramps intensity, rather than enabling full strength at a transition. Recovery
returns smoothly to clear. Recipes bound skull alpha and event/cooldown/fade
durations. Ghosts sample earlier unmodified camera frames from the same ring.
Manual modes remain under direct user control.

The director filters candidates by state, preferences, quality and cooldown,
then scores them using motion, stillness, recent events and timing. Use fixed
integer weights and a documented seeded PRNG with no external entropy in the
core. Draw from the scored candidates in bounded time. If none is eligible,
return a neutral event. Keep a fixed-size recent-event history and bounded
deadlines. Emit only bounded visual parameters and audio cues, not arbitrary
commands, script or text from a service.

A session seed comes from the host when starting or explicitly reshuffling.
The seed is not identity, a saved profile or a security token. Same seed and
same inputs reproduce output. Runtime timing and quality inputs must also be
captured for an exact test replay; a seed alone is not sufficient.

## Quality and display

Default to 320 x 240, with 160 x 120 fallback. Start with at most two expensive
temporal/geometry layers; palette and vignette remain cheap. Degrade event
complexity and layer count before resolution. Restore only after sustained
headroom. Keep processing, presentation and history-sampling time separate.
Do not claim a target FPS until measured on a named device.

The laptop mirror is fullscreen and independent of display aspect ratio.
Cover fills the display with a centered source crop and uniform scale. Contain
and nonuniform stretch are explicit maintenance options. Use checked display
dimensions, one bounded software presentation buffer and a shared transform;
do not increase core processing resolution to match the screen. Keep the
maintenance escape usable when the display or camera fails.

## Browser controls and sound

Expose recipe, independent layers, intensity, reshuffle, reduced motion and
sound consent. Show when resource limits change the requested combination.
The original silhouette sample moves, pauses and departs with reproducible
timing. It passes through the same shared processing as camera frames.

The director emits sound cues; an opt-in host adapter renders them using
original local audio or procedural nonverbal ambience. Do not use remote speech
services. Bound volume, active sources and queued cues. Mute/Stop/backgrounding
cancel all nodes and deadlines; resuming visuals does not resume sound without
a fresh permitted action. Audio failure leaves visuals usable.

Reduced motion is enforced before event selection and again in rendering so
custom recipes cannot bypass it. Disable time-varying effects and offer static
palettes/frost. Natural webcam movement remains live. Avoid rapid flashing in
all modes, including normal motion settings.

## Evidence

Use injected clocks, deterministic frames and seeds for unit/property tests.
Check malformed data and unchanged state on error; use original golden images
for layer composition and tests that fail when cooldowns or limits are removed.
Profile temporal memory separately from WASM boundary copies and native display
storage. A 2 GB VM tests the installation/resource envelope; physical laptop
camera and CPU compatibility remain separate acceptance evidence.
