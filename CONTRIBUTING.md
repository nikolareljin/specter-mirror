# Contributing

Keep changes focused and open a pull request.
Run `./check`, `python3 -m unittest discover -s tests`, and the web
build for frontend changes. Explain measured behavior and verification limits.

The core crate has no OS dependencies. Validate dimensions, byte strides and
buffer lengths before writes. Avoid per-frame allocations and unsafe Rust.
Never describe compilation as physical hardware testing.

Install the publication hooks described in [SECURITY.md](SECURITY.md) before
committing or pushing. Keep diagnostics outside Git. Use the Kiro specs for
feature requirements and record only genuine execution evidence.
