# SpecterMirror

![SpecterMirror](assets/branding/hero.svg)

Resurrecting forgotten computers into haunted interactive mirrors.

SpecterMirror is a CPU-only haunted mirror for older Linux laptops, with a
shared Rust engine and a browser interface. The native target is 32-bit antiX
with X11. Browser processing will run locally through WebAssembly and Canvas.

## Foundation

This revision validates borrowed RGB24 frames, mirrors synthetic pixels,
opens an X11 software window, and checks the portable crates for WASM.
Camera capture, haunted effects, interaction detection and browser WASM
bindings are not implemented yet. Artwork is illustrative, not camera output.

```bash
./dev --help
rustup toolchain install 1.99.1 --profile minimal --component rustfmt,clippy --target i686-unknown-linux-gnu,wasm32-unknown-unknown
./dev test
./dev native-proof
./check
cd web
npm ci
npm run build
```

Ubuntu native builds need `build-essential`, `pkg-config` and `libx11-dev`.
Full i686 checks also need `gcc-multilib` and `libc6-dev-i386`.
The X11 proof needs a display; use `xvfb-run -a ./dev native-proof` in CI.

## Documentation

- [Architecture](docs/ARCHITECTURE.md) and [effects](docs/EFFECTS.md).
- [i686 build](docs/BUILD_I686.md), [antiX installation](docs/INSTALL_ANTIX.md), [hardware](docs/HARDWARE.md).
- [WASM build](docs/BUILD_WASM.md), [web interface](docs/WEB_DEMO.md), [privacy](docs/PRIVACY.md).
- [VirtualBox](docs/VIRTUALBOX.md), [ISO toolkit](docs/ISO.md), [benchmarks](docs/BENCHMARKS.md), [demo verification](docs/DEMO.md).
- [Kiro development](docs/KIRO-DEVELOPMENT.md), [contributing](CONTRIBUTING.md), [security](SECURITY.md).
- [Asset provenance](assets/PROVENANCE.md).
- [Reused components and licenses](docs/DEPENDENCIES.md).

The planned demo destination is https://nikolareljin.github.io/specter-mirror/.
This revision does not deploy a working camera demo. The web source includes
a matching About page with links to selected public projects.

## Next steps

Build real camera filters for the native laptop and browser, then extend the
engine into a modular Smart Mirror. Halloween is the first filter theme.
Live processing and physical hardware verification must precede release claims.

Original source and SVG artwork use [Apache-2.0](LICENSE). Third-party notices
and dependency licenses remain applicable.
