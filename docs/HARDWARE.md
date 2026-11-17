# Hardware verification

| Target | Role | Verification |
| --- | --- | --- |
| Toshiba mini NB205 | Primary physical target | Exact configuration and runtime unverified |
| Dell Latitude D630 | Secondary physical target | Exact CPU, graphics, RAM, BIOS and camera unverified |
| antiX i686 VM | Reproducible sample environment | Not yet provisioned |

Run `bash scripts/discover_hardware.sh > hardware-local.txt` on each target.
The output is local diagnostic data: inspect and redact it before sharing.
Record CPU flags, OS, glibc, graphics and camera formats. Do not publish raw
device identifiers. A USB webcam is the fallback if no suitable camera exists.

The D630 commonly has a 64-bit-capable processor; the intended OS/application
environment is deliberately 32-bit. Verify installed parts rather than
describing every unit as identical. Discover supported RAM before upgrading.
Prioritize a CPU-only X11 path for Intel GMA hardware and test other variants
separately. Build success does not establish hardware compatibility.
