# SpecterMirror

![SpecterMirror](assets/branding/hero.svg)

Resurrecting forgotten computers into haunted interactive mirrors.

SpecterMirror is a CPU-only haunted mirror for older Linux laptops, with a
shared Rust engine and a browser interface. The native target is 32-bit antiX
with X11. Browser processing runs locally through WebAssembly and Canvas.

## Foundation

The browser supports Mirror, Spectral Mirror and Cursed Portrait, using shared
Rust grayscale, Sobel and Bayer processing. Combine any palette with delayed
reflection and ghost trails, or choose a named combination. Choose a webcam
or an original generated visitor scene, then use Full screen for an immersive
view. Camera access requires a button click; frames stay on your device.
The native X11 application animates a generated diagnostic sample through
the same delay/trail engine, with a 160 x 120 fallback. Native camera capture
and interaction detection remain separate work. Artwork is illustrative.

```bash
./dev --help
rustup toolchain install 1.99.1 --profile minimal --component rustfmt,clippy --target i686-unknown-linux-gnu,wasm32-unknown-unknown
./dev test
./dev all-test
./dev native-proof
./check
cd web
npm ci
cargo install wasm-pack --version 0.15.1 --locked
npm run build
```

Ubuntu native builds need `build-essential`, `pkg-config` and `libx11-dev`.
Full i686 checks also need `gcc-multilib` and `libc6-dev-i386`.
The X11 proof needs a display; use `xvfb-run -a ./dev native-proof` in CI.

## Try locally

Run `./demo native` to keep the animated native sample open until Escape.
Use the native executable's `--help` for delay, trails, resolution and reduced
motion controls; [native options](docs/NATIVE_COMPATIBILITY.md) include examples.
Run `./demo web` to preview the interface at
http://127.0.0.1:5173/specter-mirror/. Use `./demo web --port 5179` if the
default port is occupied. Install web dependencies with `npm ci --prefix web`
and wasm-pack with the command above. The helper builds WASM before serving.

`./demo vm` forwards the verified ISO and guest options to the
[QEMU runner](vm/README.md). It creates a disposable virtual disk and does not
provision the development workstation. Run `./demo --help` for complete examples.

## Documentation

- [Architecture](docs/ARCHITECTURE.md) and [effects](docs/EFFECTS.md).
- [i686 build](docs/BUILD_I686.md), [antiX installation](docs/INSTALL_ANTIX.md), [hardware](docs/HARDWARE.md).
- [WASM build](docs/BUILD_WASM.md), [web interface](docs/WEB_DEMO.md), [privacy](docs/PRIVACY.md).
- [Virtual machines](vm/README.md), [ISO media](docs/ISO.md), [benchmarks](docs/BENCHMARKS.md), [demo verification](docs/DEMO.md).
- [Kiro development](docs/KIRO-DEVELOPMENT.md), [contributing](CONTRIBUTING.md), [security](SECURITY.md).
- [Asset provenance](assets/PROVENANCE.md).
- [Reused components and licenses](docs/DEPENDENCIES.md).

The planned demo destination is https://nikolareljin.github.io/specter-mirror/.
The Pages workflow publishes verified builds from `main`; deployment must
succeed before that URL is available. The web source includes a matching
About page with links to selected public projects.

## Next steps

Connect the native laptop camera and add more effects, then extend the engine
into a modular Smart Mirror. Halloween is the first filter theme.
Live processing and physical hardware verification must precede release claims.

Original source and SVG artwork use [Apache-2.0](LICENSE). Third-party notices
and dependency licenses remain applicable.
