# Benchmarks

No physical performance measurements are published in this revision.
320 x 240 at 10-15 FPS on very old hardware, 15-24 FPS on older dual-core
hardware, and about 30 FPS in modern browsers are goals, not results.
160 x 120 is the intended fallback processing resolution.

Every result must identify the source revision, CPU/instruction flags, RAM,
OS/glibc, input format/stride, resolution, effect, duration and quality level.
Measure processing time separately from capture and display. Report dropped
frames and steady-state memory. Never infer physical performance from Xvfb.
