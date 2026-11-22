# Install the synthetic proof on antiX

This installer supports the X11 synthetic frame proof used for display and
installation validation. Installation requires `--allow-proof`.

## Build and trust a bundle

On the development workstation, link the [i686 executable](BUILD_I686.md),
then create a directory bundle:

```bash
./dev build-i686
./scripts/appliance/bundle.sh \
  --binary target/i686-unknown-linux-gnu/release/spook-kiosk \
  --runtime-packages /tmp/antix-runtime-packages \
  --output /tmp/specter-mirror-bundle
```

Record the printed SHA-256 of `SHA256SUMS` through a trusted channel. The
installer requires this external pin; an accompanying editable checksum alone
does not establish trust. Bundles contain the ELF32 x86 binary, runtime scripts,
original assets, Apache license and a complete file manifest. CPU instructions
and glibc compatibility still need target checks; cross-builds do not prove them.

## Target preparation

Install 32-bit antiX using the official interactive installer. Discover the
actual [hardware](HARDWARE.md); the standard Rust i686 target requires SSE2.
Use an existing non-root desktop user with a home under `/home`. The target
needs Bash, Python 3, `getconf`, `ldd`, X11, `wmctrl` and the executable's
runtime libraries. No Rust, Cargo or Node build toolchain is needed. Other OSes and
64-bit userspace are rejected.

Copy the bundle and `scripts/appliance/` to trusted local media. Replace HEX
with the trusted manifest checksum and mirror with the chosen account:

```bash
sudo ./scripts/appliance/install.sh --bundle /media/usb/specter-mirror-bundle \
  --sha256 HEX --user mirror --profile auto --allow-proof --offline
./scripts/appliance/verify.sh --sha256 HEX
```

`--runtime-packages` includes reviewed i386/all `.deb` archives. Prepare these
on a clean guest using the same pinned antiX base. `--offline` installs only
those archives and checks runtime libraries without network package setup.
There is no network fallback. `--install-deps` instead uses the target's
configured repositories when online runtime package setup is wanted.

Profiles `nb205`, `d630`, `virtualbox` and `generic` are explicit overrides.
`auto` recognizes NB205/D630 DMI names and otherwise selects `generic`.
The profile is installation metadata; the display proof processes a fixed
320 x 240 frame and does not access cameras.

## Startup and maintenance

Managed files live under `/opt/specter-mirror` and
`/etc/specter-mirror/config.json`. Existing configuration is preserved.
The default `--session kiosk` enables Slimski/SLiM automatic login for the selected
non-root account. The antiX target boots directly into the app without a login
prompt, normal desktop, panels or desktop autostarts. A minimal Openbox process
provides window management. Essential OS services still run. No login or
desktop setting on the development workstation is changed.

The installer supports stock antiX Slimski `atlogin_cmd` and SLiM `login_cmd`, and refuses an
unrecognized display manager configuration. It saves the original configuration
and restores it during uninstall. Later operator edits require reconciliation
before reinstall/uninstall so they are not overwritten. There is no systemd
dependency. Changing the kiosk account or session mode requires uninstall first.

Use `--no-autologin` only if a login prompt is desired. The optional
`--session desktop` adds one marked startup block to the selected user's
`~/.desktop-session/startup` while preserving the normal desktop; it does not
change automatic login. This is an explicit alternative, not the appliance
default.

After desktop login, the launcher runs as that user and requests fullscreen
with `wmctrl` for the application's process. Escape exits without restarting.
Crashes cause at most three attempts with backoff. Local logs contain start/exit
events only, rotate at 64 KiB and retain one previous file. Application output
is discarded; no camera frames are recorded.

As the desktop user:

```bash
./scripts/appliance/maintenance.sh disable
./scripts/appliance/maintenance.sh status
./scripts/appliance/maintenance.sh enable
```

Ctrl+Alt+Escape opens a maintenance terminal in the dedicated session. Escape
or a failed app leaves the minimal session available for that key instead of
entering an automatic-login restart loop. Ctrl+Alt+F1 also permits console
recovery using the account credentials set during antiX installation.

Disable requests a running session to exit and prevents future launches.
Enable permits the next login launch; it does not start a process itself.
Before uninstall, disable the running session, then:

```bash
sudo ./scripts/appliance/uninstall.sh --sha256 HEX
```

Uninstall verifies the installed manifest, removes the managed startup block,
application and system configuration, and preserves user state and unrelated
files. If managed files changed, inspect them locally before removal.

## Staging tests

`--root /absolute/test-root` stages filesystem changes only. It does not
install packages, run the binary, check CPU/glibc or prove reboot startup.
The staging tree must contain antiX's release marker or distribution ID and a suitable
`etc/passwd` account. Tests create disposable fixtures; do not use a personal
installation as a staging target.

See the [VM guide](../vm/README.md) and [ISO tooling](ISO.md) for installation
and reboot validation. Compilation and staging do not establish physical
hardware compatibility.
