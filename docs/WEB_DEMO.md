# Web interface

The Vite/TypeScript source contains a themed interface shell and matching
About page. It does not request a camera or load WASM in this revision.
The mirror artwork is an original illustration, not a processed sample.

```bash
cd web
npm ci
npm run dev
npm run build
```

The eventual demo requires HTTPS and explicit user interaction before camera
permission. Process visitor frames locally and render to Canvas 2D. Provide
stop-camera controls, intensity/effect selection, measured FPS and a sample
fallback for denied or unavailable cameras. Stop media tracks when leaving.
Use the visitor's camera in the browser and the laptop's camera natively.

GitHub Pages project hosting must retain `/specter-mirror/` for direct About
navigation and all asset/WASM URLs. Do not enable deployment until the
integrated demo works anonymously and permission/error paths are verified.
