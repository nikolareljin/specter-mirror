# SpecterMirror installation media

This media contains the Rust synthetic frame proof for display validation.
Check `PROVENANCE.json`: `appliance-preview` embeds the application and its
runtime packages in the live filesystem; `synthetic-proof-toolkit` retains
the original live filesystem and requires manual provisioning after install.
Both modes preserve antiX's official interactive disk installer.

## Appliance media

1. Verify the complete ISO checksum against your trusted build record.
2. Boot the media. The official installer opens automatically.
3. Select the destination disk, review the erasure confirmation and create
   your account. Keep automatic login selected and retain the account password
   for maintenance and console recovery.
4. Reboot from the installed disk. The dedicated session starts the Rust app.

Ctrl+Alt+Escape opens a maintenance terminal. Escape exits the display proof
while preserving that recovery key. No normal desktop autostarts run in the
dedicated session. The application runs without root privileges.

## Manual toolkit installation

1. Verify the complete ISO checksum against your trusted build record.
2. Boot antiX and use its official interactive installer. Choose a desktop
   account and retain its password for maintenance and console recovery.
3. Boot the installed antiX system and mount this media.
4. Open a terminal in the `specter-mirror` directory on this media.
5. Read the bundle manifest pin in `PROVENANCE.json` and compare it to the
   trusted bundle/build record. Replace HEX and mirror below.

```bash
sudo bash scripts/appliance/install.sh --bundle "$PWD/bundle" \
  --sha256 HEX --user mirror --allow-proof --install-deps
bash scripts/appliance/verify.sh --sha256 HEX
```

Use `--offline` instead of `--install-deps` when the bundle contains the
reviewed runtime package archives for this exact antiX release, or all required
libraries and tools are already installed. Provisioning requires Python 3,
Bash, 32-bit antiX, SSE2 and runtime libraries compatible with the binary.

The default installation configures Slimski or SLiM automatic login and a dedicated
session for the selected account on this antiX system. It bypasses normal
desktop startup. Reboot after provisioning. Ctrl+Alt+Escape opens maintenance;
Escape exits the proof while preserving that recovery key.

As the desktop user, disable the application before uninstall:

```bash
bash scripts/appliance/maintenance.sh disable
sudo bash scripts/appliance/uninstall.sh --sha256 HEX
```

The application runs without root privileges. Uninstall restores the original
display manager configuration if it has not been edited since provisioning. User data
remains. No Rust or Node build toolchain is needed to run the prebuilt proof.
