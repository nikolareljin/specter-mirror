# antiX virtual appliance tools

The tools provision the current synthetic frame proof. They do not yet
provide a working haunted webcam appliance. See [installation](../docs/INSTALL_ANTIX.md)
and [ISO scope](../docs/ISO.md) before creating media.

## Create a disposable guest

Download and verify the 32-bit full image from the [official antiX page](https://antixlinux.com/download/).
The recorded base is antiX-26.1_386-full.iso, released September 29, 2026.
Its published SHA-256, checked November 19, 2026, is:

```text
6ec6f6522c69b4411d32f61e853709cdaf94df709c264b2a4d7de9724ce57fbc
```

```bash
./vm/create.sh --name specter-mirror-test --iso /tmp/antiX-26.1_386-full.iso \
  --iso-sha256 6ec6f6522c69b4411d32f61e853709cdaf94df709c264b2a4d7de9724ce57fbc \
  --directory /tmp/specter-mirror-vm
VBoxManage startvm specter-mirror-test --type gui
```

Creation uses BIOS, one CPU, 1024 MB RAM, a dynamically allocated 16 GB IDE
disk and software graphics. It refuses an existing VM name or directory.
It never starts a VM, installs the OS or erases a disk. On failure it retains
partial state for inspection. The host must have a working VirtualBox kernel
driver. Install antiX through its official interactive installer and create a
non-root desktop account. The shared installer configures automatic login
and the dedicated app session on this antiX target by default.

Transfer the verified bundle and `scripts/appliance/` into the guest using
removable media or the [toolkit ISO](../docs/ISO.md). Run `vm/install.sh` or
`vm/setup-kiosk.sh` with the shared installer arguments, then
`vm/verify.sh --sha256 HEX`. These wrappers contain no provisioning logic.
`--help` prints each script's header; `--options` lists complete arguments.

## Guest verification

1. Install twice with `--allow-proof`; compare configuration and startup files.
2. Disconnect networking and repeat with `--offline`. All runtime dependencies
   must already exist; a complete dependency bundle is pending.
3. Reboot and confirm automatic login into one fullscreen synthetic frame
   window, with no normal desktop applications. Escape exits without restarting;
   Ctrl+Alt+Escape must still open maintenance. Reboot and repeat.
4. As the desktop user, disable startup with `scripts/appliance/maintenance.sh disable`.
   Confirm the running proof exits and the next desktop login remains usable.
5. Enable startup again, then test uninstall in this disposable guest. Confirm
   unrelated startup commands and user files remain intact.

These are operator acceptance steps, not claims of executed VM testing.
Automated tests cover staging, negative inputs, repeat provisioning,
rollback, bounded restarts and ISO payload/boot metadata preservation.
They do not boot a real antiX guest. Camera capture, all effects and physical
NB205/D630 compatibility remain separate application and hardware work.

## USB camera and export

After the native camera pipeline exists, attach a compatible USB webcam using
VirtualBox USB filters and inspect `/dev/video*` inside the guest. Host cameras
are not automatically V4L2 devices. Check host permissions and the licenses
of optional VirtualBox components before installing or redistributing them.

Before an OVA export, use a separate disposable guest: remove credentials,
SSH host keys, account histories, personal footage, machine identifiers and
shared folders; inspect the powered-off image. Check package and asset
redistribution terms. Export only a tested guest with `VBoxManage export`.
Do not commit ISOs, virtual disks or OVA files. Keep these under `vm/local/`
or outside the checkout until a reviewed release is ready.

## Troubleshooting

| Failure | Action |
|---|---|
| VirtualBox kernel driver missing | Repair the host installation for the running kernel; do not claim a guest test |
| ELF32/SSE2/loader failure | Check the exact CPU, 32-bit userspace and native glibc build baseline |
| Missing runtime library offline | Install matching antiX runtime packages before disconnecting networking |
| Window is not fullscreen | Check IceWM/EWMH and `wmctrl -lp`; Escape and maintenance remain available |
| No automatic startup | Check `~/.desktop-session/startup` and antiX desktop-session settings |
| Application repeatedly exits | Startup stops after three attempts; inspect the user's local session log |
