# Effects

The browser processes camera or generated visitor frames with shared Rust code.
Spectral edges and portrait dithering can be combined with reflection delay
and ghost trails. Temporal processing precedes palettes and mirrors once.
Automatic haunting starts clear and gradually increases these layers using
the shared motion/state progression. Manual controls remain available when
automatic mode is disabled. Head localization and skull overlays are not
provided by this progression.

| Effect | Current behavior |
| --- | --- |
| Spectral Mirror | Grayscale edges and spectral palette |
| Cursed Portrait | Bayer dithering and bone/midnight palette |
| Delayed reflection | Selects a past frame, up to 600 ms requested delay |
| Ghost Trails | Blends the base with a frame approximately 200 ms older |

History samples at 100 ms intervals and holds at most eight frames within
4 MiB. Short or sparse history falls back to its oldest frame; exact requested
delay is not guaranteed. Strength zero disables trails, delay zero uses the
current frame, and both can be combined. Settings changes reset history.
Reduced motion suppresses both temporal layers. Skeleton contours, localized
distortion, analog treatments and motion-triggered event selection remain
requirements in the haunted-experience specification.

The target interaction contract is IDLE -> PRESENCE_DETECTED -> TRANSFORMING ->
HAUNTED -> RECOVERING -> IDLE. Motion is sufficient; face detection stays
optional. Seed effects and inject elapsed time for repeatable tests.
Reduced-motion mode must avoid flicker, jumps and excessive temporal motion.
