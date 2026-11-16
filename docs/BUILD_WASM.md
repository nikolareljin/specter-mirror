# Build for WebAssembly

```bash
rustup target add --toolchain 1.99.1 wasm32-unknown-unknown
./dev wasm-check
```

This checks the shared engine and binding-crate scaffold for the WASM target.
It does not create a JavaScript interface or prove browser execution.
Actual bindings and wasm-pack integration must preserve the shared core's
buffer contract and return errors without panicking on malformed input.

The Vite shell builds separately with `cd web && npm ci && npm run build`.
The project base path is `/specter-mirror/`. Pages deployment is enabled only
after working WASM, permission handling and sample fallback are verified.
