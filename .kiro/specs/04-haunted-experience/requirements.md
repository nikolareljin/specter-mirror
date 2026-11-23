# Haunted interaction requirements

These requirements describe the target experience. Unchecked tasks are not
claims about features available in the current application.

## Interaction

1. Given elapsed time, a seed and motion observations, the shared Rust engine
   shall transition through IDLE, PRESENCE_DETECTED, TRANSFORMING, HAUNTED and
   RECOVERING without a wall clock, network or OS dependency.
2. Presence estimation shall use configurable motion thresholds, hysteresis
   and a stillness grace period. Motion alone shall not be labelled as face,
   person, identity, gaze or departure detection.
3. Stop, camera loss and source changes shall clear temporal state, reset the
   interaction and cancel pending visual/audio events. Backgrounding shall
   stop audio and suspend processing; resuming shall reset temporal state.
4. The experience shall begin with a mostly normal reflection, build tension,
   run bounded events and recover. No event shall bypass the user's settings.

## Independent effects and recipes

5. Provide independent delay, ghost trails, ghost/skull apparitions, spectral
   and portrait palettes, localized distortion, frost, vignette and atmospheric
   messages. Static artwork shall not replace processing of camera frames.
6. Provide Haunt me, named recipes and custom combinations. A recipe shall
   define eligible layers, intensity ceilings, timing ranges and resource
   limits. Preserve the existing three manual processing modes.
7. Apply temporal layers, spatial distortion, geometry, palette and presentation
   treatments in a documented order. Reject invalid recipe configuration before
   mutation. Unsupported or costly combinations shall degrade predictably and
   expose the effective combination rather than silently ignoring controls.
8. Automatic sessions shall use a fresh seed. Replaying the same seed, frames,
   elapsed times, preferences and quality inputs shall reproduce decisions and
   output. New seeds shall allow distinct sequences, not guarantee uniqueness.

## Adaptive event selection

9. A bounded local director shall score eligible events using motion amount
   and location, stillness, time since an event, recent choices and measured
   processing cost. Choose among eligible scores with seeded randomness.
10. Enforce event durations, cooldowns, intensity and simultaneous-layer limits.
    Avoid consecutive repeats when eligible alternatives exist. An empty set
    shall yield a neutral reflection, not an error or unbounded retry loop.
11. Place localized effects using normalized motion coordinates transformed
    consistently with the displayed frame. Do not claim face alignment.
12. The director shall require no downloaded model, server, GPU or additional
    worker pool. Use the same policy in native and WASM applications.

## Legacy hardware and memory

13. Support portable i686 and wasm32 builds with CPU-only processing. Target
    2 GB and 4 GB antiX installations; actual hardware performance requires
    measurement. Begin at 320 x 240 and support 160 x 120 fallback.
14. Temporal pixel history shall use at most 4 MiB and eight retained frames.
    Allocate and reuse storage outside the frame loop. Timestamp bounded
    history samples so delay is independent of display FPS. Warm-up shall use
    available frames without reading uninitialized data.
15. Drop stale camera frames and retain no unbounded queue, event history or
    per-frame allocations in the shared processing loop. Document unavoidable
    copies at the WASM boundary separately from core memory measurements.
16. The default quality tier shall permit at most two costly temporal/geometry
    layers alongside cheap treatments. Sustained frame-cost overruns shall
    reduce layer count/complexity, then resolution. Restoration shall require
    sustained headroom and shall not oscillate frame by frame.
17. Validate dimensions, strides, buffer lengths, arithmetic and configuration
    before state mutation or output writes. Resizing shall clear incompatible
    frame history. Measured timings shall distinguish processing and display.

## Presentation and consent

18. The installed laptop shall display an edge-to-edge fullscreen mirror at
    wide, tall and square aspect ratios. Default cover shall scale uniformly
    and crop overflow without black bars. Maintenance settings shall also
    provide contain and nonuniform stretch. Processing size shall remain
    independent of display size; native scaling shall remain CPU-only.
19. Crop, scale and horizontal reflection shall apply consistently to pixels
    and effect positions, exactly once. Check presentation buffer arithmetic
    and enforce a documented buffer budget on resize. Keep maintenance and
    immediate Stop controls available without showing desktop chrome in the
    normal appliance view.
20. Reduced motion shall suppress delay, trails, animated warps, breathing,
    creeping frost, abrupt apparitions and sample movement. Offer static
    treatments; do not freeze or falsely label live webcam input.
21. Avoid rapid flashing in every mode. Optional quiet sound shall require a
    dedicated user action, have a bounded volume and immediate mute, and stop
    on Stop, pagehide, backgrounding or capture failure. Cancel scheduled audio
    as well as active sources. No microphone or remote speech service.
22. Supply original local sound or procedural nonverbal whisper-like ambience.
    A sound failure shall not stop the visual experience. Audio shall start
    only within a permitted browser gesture and shall never auto-resume.
23. Replace the abstract sample with an original procedural human silhouette
    that moves, pauses and departs through the same processing pipeline. Label
    it as generated, not a recording or evidence of a real webcam test.
24. Maintain keyboard operation, labelled controls, visible effective settings,
    responsive layout and restrained status announcements. Preserve matching
    About styling and the existing public project destinations.

## Verification

25. Test clock boundaries, debounce, stillness grace, recovery, camera loss,
    reset, huge time jumps, deterministic seeds and empty event sets. Property
    tests shall check invariants without claiming mathematical proof.
26. Test each layer and supported combination, invalid recipes, cooldown and
    concurrency limits, warm-up and bounded storage. Use original golden frames
    at 320 x 240 and 160 x 120. Test crop/mirror alignment on multiple aspects.
27. Verify sound consent/cleanup, reduced motion, permission errors, responsive
    controls and generated-sample behavior. Regenerate and inspect the Pages
    artifact manifest for browser changes before publication.
28. Record host, WASM and i686 compilation separately from runtime evidence.
    Measure a 2 GB antiX guest and the physical laptop before claiming target
    compatibility, frame rate, RAM usage or webcam support.
