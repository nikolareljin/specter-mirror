# Build for i686

On an Ubuntu x86_64 development machine:

```bash
sudo apt-get install build-essential pkg-config libx11-dev gcc-multilib libc6-dev-i386 libclang-dev linux-libc-dev
rustup target add --toolchain 1.99.1 i686-unknown-linux-gnu
./dev i686-check
```

The camera backend (`v4l`, through `v4l2-sys-mit`) generates its bindings with
bindgen while building. That needs libclang and `linux/videodev2.h` from
`linux-libc-dev`; the i686 build also reads the multilib headers above. The
host that produced the recorded results had `libclang1-18`, `clang-18` and
`linux-libc-dev` 6.8.0 installed. CI explicitly installs `libclang-dev` and
`linux-libc-dev` alongside the native prerequisites. Nothing from libclang is
linked into the executable.

The pinned compiler is selected by `./dev`; the script does not change the
machine's default compiler. `cargo check` does not link a runnable executable.
Link a release using `./dev build-i686`, which selects the pinned compiler.

A linked release build on the Ubuntu 24.04 x86_64 host (`./dev build-i686`)
produced an ELF 32-bit Intel 80386 PIE executable. Its dynamic section lists
only `libgcc_s.so.1`, `libc.so.6` and `ld-linux.so.2`, with interpreter
`/lib/ld-linux.so.2` and symbol versions up to GLIBC_2.34 from the build host.
No V4L, libv4l or X11 library is linked directly (minifb loads X11 at run
time). This is a link and ELF inspection only. It does not show that the
binary runs on antiX, that its glibc baseline fits the target or that the CPU
has SSE2, and it never opened a camera.

Prefer a compatible 32-bit build environment for release artifacts. Build
natively there with the same pinned compiler, source revision and lockfile,
then inspect `file`, `ldd`, ELF interpreter and required GLIBC versions.
Test the actual executable on antiX and physical targets before release.

The standard Rust i686 target can require SSE2. Do not claim compatibility
with CPUs lacking those instructions, or assume a compiler flag can remove
instructions already present in the distributed standard library. Discover
CPU flags before selecting a release target or toolchain.
