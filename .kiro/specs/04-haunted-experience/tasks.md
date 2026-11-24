# Haunted interaction tasks

Execute one bounded task at a time. Check off work only after implementation
and verification. Keep execution evidence in `docs/KIRO-DEVELOPMENT.md`.

- [x] Add validated motion observations with normalized changed-pixel location,
  threshold hysteresis, stillness grace and a clock-driven five-state machine.
  Test reset, malformed input, time boundaries and huge time jumps.
- [x] Add one bounded timestamped frame-history ring shared by delayed
  reflection and ghost trails. Verify warm-up, reset, reuse and the 4 MiB cap.
- [ ] Add independent procedural ghost/skull geometry, distortion, frost,
  vignette and palettes, with original golden fixtures at both processing sizes.
- [ ] Add validated recipe composition and deterministic order; test independent
  layers, combinations, invalid configuration and effective quality limits.
- [ ] Add seeded adaptive event selection with cooldowns, recent-event limits,
  motion positioning, stillness cues and neutral fallback. Test deterministic
  replay and meaningful sequence variation across fixed test seeds.
- [ ] Integrate Haunt me, recipes, custom layers, reshuffle and the original
  silhouette sample through WASM. Preserve camera cleanup and manual modes.
- [ ] Add opt-in local audio with bounded volume/cues and complete cleanup;
  verify sound never starts or resumes automatically.
- [ ] Enforce reduced motion and no rapid flashing across every combination;
  test runtime preference changes and source/camera loss.
- [ ] Add fullscreen native cover/contain/stretch presentation, consistent
  crop/mirror coordinates, checked display allocation and maintenance access.
- [ ] Add measured adaptive quality and regression/property tests. Record host,
  WASM and i686 build evidence separately from a 2 GB antiX guest and physical
  laptop webcam/resource measurements.
