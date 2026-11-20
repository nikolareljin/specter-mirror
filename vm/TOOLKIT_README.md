# SpecterMirror toolkit preview

This media contains a synthetic frame proof, not the completed webcam app.
It does not replace antiX's interactive disk installer or alter the live root.

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

Use `--offline` instead of `--install-deps` only if all runtime packages are
already present. The current media does not include a complete dependency
package set. The installer requires Python 3, Bash, 32-bit antiX, SSE2 and
runtime libraries compatible with the prebuilt binary.

The default installation configures SLiM automatic login and a dedicated
session for the selected account on this antiX system. It bypasses normal
desktop startup. Reboot after provisioning. Ctrl+Alt+Escape opens maintenance;
Escape exits the proof while preserving that recovery key.

As the desktop user, disable the application before uninstall:

```bash
bash scripts/appliance/maintenance.sh disable
sudo bash scripts/appliance/uninstall.sh --sha256 HEX
```

The application runs without root privileges. Uninstall restores the original
SLiM configuration if it has not been edited since provisioning. User data
remains. No Rust or Node build toolchain is needed to run the prebuilt proof.
