# Effects

The current core mirrors synthetic RGB24 frames only. Haunted effects are
not implemented in this foundation.

| Intended effect | Processing direction |
| --- | --- |
| Spectral Mirror | Grayscale edges and spectral palette |
| Skeleton Awakening | Procedural skull contours |
| Cursed Portrait | Dithering and muted portrait palette |
| Poltergeist | Seeded distortion and delayed reflection |
| Ghost Trails | Bounded temporal buffers |
| Analog Haunting | Restrained scanline and signal distortion |

The interaction contract is IDLE -> PRESENCE_DETECTED -> TRANSFORMING ->
HAUNTED -> RECOVERING -> IDLE. Motion is sufficient; face detection stays
optional. Seed effects and inject elapsed time for repeatable tests.
Reduced-motion mode must avoid flicker, jumps and excessive temporal motion.
