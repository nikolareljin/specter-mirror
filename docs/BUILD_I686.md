# Build for i686

On an Ubuntu x86_64 development machine:

```bash
sudo apt-get install build-essential pkg-config libx11-dev gcc-multilib libc6-dev-i386
rustup target add --toolchain 1.99.1 i686-unknown-linux-gnu
./dev i686-check
```

The pinned compiler is selected by `./dev`; the script does not change the
machine's default compiler. `cargo check` does not link a runnable executable.
Link a release using `./dev build-i686`, which selects the pinned compiler.

Prefer a compatible 32-bit build environment for release artifacts. Build
natively there with the same pinned compiler, source revision and lockfile,
then inspect `file`, `ldd`, ELF interpreter and required GLIBC versions.
Test the actual executable on antiX and physical targets before release.

The standard Rust i686 target can require SSE2. Do not claim compatibility
with CPUs lacking those instructions, or assume a compiler flag can remove
instructions already present in the distributed standard library. Discover
CPU flags before selecting a release target or toolchain.
