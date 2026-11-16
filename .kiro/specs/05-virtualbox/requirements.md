# Appliance and VirtualBox requirements

1. Provision a clean antiX i686 target twice without corrupting configuration; require runtime dependencies only and support sample fallback.

2. Use the same installer on physical hardware and VirtualBox, with maintenance, uninstall and clear error exits; do not require systemd.

3. Verify reboot startup and a complete offline bundle in a clean guest; release an ISO/OVA only after boot, installer and privacy validation.
