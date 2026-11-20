# antiX installation media

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

## Full appliance remaster, later phase

antiX distinguishes [live remastering](https://download.tuxfamily.org/antix/docs-antiX-21/FAQ/remaster.html)
from an installed-system snapshot. Its official
[iso-snapshot source](https://github.com/antiX-Linux/iso-snapshot) creates live
media from a running system. Use supported tools from the selected antiX
release inside a clean guest; do not assume old upstream configuration
schemas match the installed package.

After the native application and complete runtime bundle exist:

1. Install the pinned 32-bit full image in a disposable guest.
2. Provision with the shared installer and test without networking.
3. Apply original spooky artwork to the boot menu, live desktop, installer
   presentation and startup using that release's supported customization
   points. Preserve official disk choices and confirmations.
4. Create a generic distributable snapshot with the guest's official tool.
   Review exclusions, accounts, passwords, host keys, histories and machine
   identifiers. Do not snapshot a personal workstation.
5. Boot the candidate, test sample mode and maintenance, run the official
   installer into a second disposable disk and reboot the installed guest.
6. Repeat from a clean base and explain output differences before claiming
   reproducibility.

Record package versions/sources, base checksum, snapshot tool version,
scripted changes, dependency licenses and redistribution notices. Original
application code is Apache-2.0; antiX packages retain their own licenses.
Resolve distribution obligations before public ISO/OVA publication.

No destructive unattended disk installer is provided. Full remastering,
installation theming and release publication require working application,
live boot, installer and installed-system validation. Keep large media outside
Git and distribute only validated artifacts through releases.
