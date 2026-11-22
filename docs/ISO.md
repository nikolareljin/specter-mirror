# antiX installation media

## Appliance ISO

Use the pinned 32-bit antiX full image below. Build the i686 application and
prepare the runtime package archives on a clean guest of that exact release:

```bash
sudo apt-get update
sudo apt-get --download-only install openbox xterm
```

Transfer the reviewed archives from `/var/cache/apt/archives` to the build
workstation, then create the bundle with `--runtime-packages` as described in
[installation](INSTALL_ANTIX.md). Keep the printed bundle manifest checksum.

The full builder requires Docker on an x86 Linux build host with 32-bit
executable support. The container extracts the verified base, installs only
the bundled runtime packages without networking, configures the dedicated
session and creates a new SquashFS filesystem. Host login and desktop settings
are not modified. Build context is restricted to the tool image definition.

```bash
docker build -t specter-mirror-iso-builder:local vm/iso-builder
./vm/build-iso.sh --base /tmp/antiX-26.1_386-full.iso \
  --base-sha256 6ec6f6522c69b4411d32f61e853709cdaf94df709c264b2a4d7de9724ce57fbc \
  --bundle /tmp/specter-mirror-bundle --bundle-sha256 HEX \
  --output /tmp/specter-mirror-appliance.iso --allow-proof --appliance
```

Booting appliance media opens the official interactive antiX installer. Disk
selection and erasure confirmation remain mandatory. Automatic login is
preselected using the installer's `User/Autologin` setting. The kiosk follows
the account selected during installation. After reboot it starts the Rust
application without normal desktop autostarts or a login prompt. Maintenance
remains accessible with Ctrl+Alt+Escape. The privileged installer entry checks
for mounted live media and refuses execution from an installed system.

The ISO contains the app and its runtime dependencies; the target needs no
Rust, Cargo, Node, Docker or hypervisor. The current display validation binary
requires `--allow-proof`. Validate the resulting image in a fresh offline
guest before using it on physical hardware.

## Toolkit preview ISO

`vm/build-iso.sh` adds a verified proof bundle and installer tools under
`/specter-mirror` on an existing bootable antiX ISO. It checks external
SHA-256 pins for the base and bundle, refuses existing output, replays the
base's boot settings using xorriso, and verifies the embedded manifest.
It does not modify the live root filesystem, kernel or official installer.

```bash
./vm/build-iso.sh --base /tmp/antiX-26.1_386-full.iso \
  --base-sha256 6ec6f6522c69b4411d32f61e853709cdaf94df709c264b2a4d7de9724ce57fbc \
  --bundle /tmp/specter-mirror-bundle --bundle-sha256 HEX \
  --output /tmp/specter-mirror-toolkit-preview.iso --allow-proof
```

Build prerequisites are Python 3 and xorriso, with enough space for both ISO
images. The base checksum is from the [official antiX downloads](https://antixlinux.com/download/),
checked November 19, 2026. Reverify before downloading. Do not substitute a
different base without reviewing architecture, package baseline and checksums.

This media boots the normal antiX desktop. Install antiX interactively, then
mount the media and run the shared installer from
`/specter-mirror/scripts/appliance/` with the verified bundle there.
The toolkit is not automatically copied into the installed system.
This is toolkit media, not the finished boot-to-mirror appliance ISO.

`PROVENANCE.json` records the base and bundle hashes and that the root
filesystem is unchanged. Record local tool versions, source revision, output
checksum and actual VM tests with each candidate. Do not publish an image
solely because the builder returned success.

## Release validation

antiX distinguishes [live remastering](https://download.tuxfamily.org/antix/docs-antiX-21/FAQ/remaster.html)
from an installed-system snapshot. Its official
[iso-snapshot source](https://github.com/antiX-Linux/iso-snapshot) creates live
media from a running system. Use supported tools from the selected antiX
release inside a clean guest; do not assume old upstream configuration
schemas match the installed package.

Before distributing an appliance image:

1. Boot the candidate without networking and confirm the official installer.
2. Install into a new disposable disk, then reboot from that disk and confirm
   automatic login, fullscreen application startup and maintenance access.
3. Exercise repeat provisioning, checksum refusal, recovery and uninstall.
4. Record base/bundle/output hashes, tool versions and actual hardware tests.
5. Preserve all source, library, asset and distribution notices; resolve their
   redistribution requirements before publishing.
6. Repeat from a clean base and explain differences before claiming reproducibility.

Record package versions/sources, base checksum, snapshot tool version,
scripted changes, dependency licenses and redistribution notices. Original
application code is Apache-2.0; antiX packages retain their own licenses.
Resolve distribution obligations before public ISO/OVA publication.

No destructive unattended disk installer is provided. Keep large media outside
Git and distribute only validated artifacts through releases. Installation
theming must preserve the official installer's disk choices and confirmations.
