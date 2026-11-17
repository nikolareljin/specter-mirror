# Install on antiX

The application is not appliance-ready yet. Install antiX using its official
interactive installer, then verify architecture, CPU flags, X11 and camera
availability with [hardware discovery](HARDWARE.md).

The current synthetic proof can be built in a compatible development
environment using [BUILD_I686.md](BUILD_I686.md). Run the linked executable
from an X11 session. Escape closes the window; `--self-test` renders once.

The eventual release bundle will contain a prebuilt executable, runtime
dependencies, samples, assets and checksums. Target devices will not need
Rust, Node or Kiro. One shared idempotent installer will support hardware and
VMs, with sample fallback, maintenance and uninstall. No systemd dependency
or destructive unattended disk installation is permitted.

Themed antiX install media is a later extension. It must preserve the
official installer's interactive behavior and undergo boot/install validation.
