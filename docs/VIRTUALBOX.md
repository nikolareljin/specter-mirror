# VirtualBox

Use a clean 32-bit antiX guest with X11. No VM appliance or OVA has been
validated yet. The shared installer will be the single provisioning entry
point for both VM and physical targets; no development toolchain is required
in the eventual target image.

Sample mode must be the default reliable VM test, without a webcam. USB
passthrough is optional and depends on host/guest/device support. A built-in
host webcam is not automatically a guest V4L2 device.

Repeat installation, reboot into sample mode, test maintenance and uninstall,
and verify offline bundles with networking disabled. Export an OVA only after
removing credentials, account data and machine-specific secrets and checking
redistribution licenses. Never commit VM images to Git.
