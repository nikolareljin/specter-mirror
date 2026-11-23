# Build for WebAssembly

```bash
rustup target add --toolchain 1.99.1 wasm32-unknown-unknown
./dev wasm-check
cargo install wasm-pack --version 0.15.1 --locked
npm ci --prefix web
npm --prefix web run build
```

`wasm-check` checks compilation only. The npm build runs wasm-pack, creates
JavaScript bindings in `web/generated/`, type-checks the frontend and builds
both HTML pages and their WASM assets into `web/dist/`. Generated output is
ignored. Add the standard Cargo bin directory to PATH if wasm-pack is not found.

Run `./demo web` for the local browser demo. See [web interface](WEB_DEMO.md)
for modes and lifecycle behavior. The project base path is `/specter-mirror/`.
Pages deployment remains a separate verification step.
