# VirtualBox

The [VM guide](../vm/README.md) provides disposable guest creation, shared
provisioning wrappers, verification steps, USB camera guidance and OVA export
requirements. The [installer](INSTALL_ANTIX.md) currently supports an opt-in
synthetic frame proof. No antiX guest or OVA has been validated yet.

Automated tests stage files without privilege and exercise idempotency,
checksum failures, symbolic-link refusal, rollback, maintenance and bounded
restarts. The ISO test preserves an El Torito boot image and checks the bundle
payload in generated media. These tests do not establish guest boot, physical
compatibility, live camera functionality or measured performance.
