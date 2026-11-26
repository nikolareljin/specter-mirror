# Reused components

| Component | Reference | Use | License |
| --- | --- | --- | --- |
| minifb | 0.29.0, Cargo.lock | X11 software window (native application; development-only preview window for spook-web) | MIT OR Apache-2.0 |
| rustface | 0.1.7, Cargo.lock | Local frontal face detection in the development preview only; dev-dependency of spook-web, not in WASM or kiosk builds | BSD-2-Clause |
| num, byteorder | Cargo.lock, through rustface | Numeric support for rustface in the preview | MIT OR Apache-2.0 |
| Vite | 8.3.8, web/package-lock.json | Frontend build | MIT |
| TypeScript | 7.0.2, web/package-lock.json | Frontend type checking | Apache-2.0 |
| wasm-bindgen | 0.2.130, Cargo.lock | Rust/JavaScript bindings for the WASM mirror engine | MIT OR Apache-2.0 |
| wasm-pack | 0.15.1, development tool | Builds web/generated bindings; not a runtime dependency | MIT OR Apache-2.0 |
| ci-helpers | production | Reusable build, test and security workflows | MIT |
| script-helpers | Installed development tooling | Local publication hook installation | MIT |
| script-helpers | 548597c18f70c010a289c29305c45fe84863d12c | Pages publication checks and pinned scanner installation, outside the application | MIT |
| antiX | 26.1, 32-bit full image | Appliance base and official interactive installer | Distribution packages retain their own licenses |
| Openbox, wmctrl, xterm | Runtime packages from the selected antiX release | Dedicated X11 session and maintenance terminal | Preserve each package's copyright notices |
| QEMU | Build host installation | Primary disposable VM validation | GPL-2.0 and component notices |
| VirtualBox | Optional build host installation | Secondary VM validation | Check the installed edition and optional extension licenses |
| Docker, Debian, squashfs-tools, xorriso | Pinned builder base and host tools | Isolated filesystem and ISO preparation | Preserve applicable component licenses |

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
[rustface](https://github.com/atomashpolskiy/rustface/blob/master/LICENSE),
[Vite](https://github.com/vitejs/vite/blob/main/LICENSE),
[TypeScript](https://github.com/microsoft/TypeScript/blob/main/LICENSE.txt),
[ci-helpers](https://github.com/nikolareljin/ci-helpers/blob/main/LICENSE),
[script-helpers](https://github.com/nikolareljin/script-helpers/blob/main/LICENSE).

Preserve applicable notices in release bundles and inventory transitive
dependencies from the lockfiles. No third-party source is copied here merely
to use a reusable workflow.
