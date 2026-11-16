# Reused components

| Component | Reference | Use | License |
| --- | --- | --- | --- |
| minifb | 0.29.0, Cargo.lock | X11 software window | MIT OR Apache-2.0 |
| Vite | 8.3.8, web/package-lock.json | Frontend build | MIT |
| TypeScript | 7.0.2, web/package-lock.json | Frontend type checking | Apache-2.0 |
| ci-helpers | production | Reusable build, test and security workflows | MIT |
| script-helpers | Installed development tooling | Local publication hook installation | MIT |

The application source and original SVG artwork are new. The libraries and
build tooling above are reused components, not newly developed application
features. No pre-existing camera/effect application has been incorporated.
The publication tooling is not a runtime or standalone-build dependency.

The CI workflows resolve `ci-helpers@production`; the reference inspected on
2026-11-16 was `aff7f54945c665099d298c64cf6456056f088bff`. The production
reference can advance through that library's release process. Record the
resolved revision used by each judged release rather than treating the
reference as immutable. SpecterMirror does not move the shared reference.

Upstream workflow actions remain inside ci-helpers. Application workflow
callers reference the shared library rather than direct third-party actions.
CI and local publication checks have different roles; inbound CI cannot
prevent an initial upload.

Upstream licenses:
[minifb](https://github.com/emoon/rust_minifb),
[Vite](https://github.com/vitejs/vite/blob/main/LICENSE),
[TypeScript](https://github.com/microsoft/TypeScript/blob/main/LICENSE.txt),
[ci-helpers](https://github.com/nikolareljin/ci-helpers/blob/main/LICENSE),
[script-helpers](https://github.com/nikolareljin/script-helpers/blob/main/LICENSE).

Preserve applicable notices in release bundles and inventory transitive
dependencies from the lockfiles. No third-party source is copied here merely
to use a reusable workflow.
