#!/usr/bin/env bash
# SCRIPT: vm/qemu.sh
# DESCRIPTION: Boot a disposable antiX validation guest using QEMU and KVM or TCG.
# USAGE: ./vm/qemu.sh [options]
# PARAMETERS:
#   --iso PATH          Verified antiX installation ISO.
#   --iso-sha256 HEX    Trusted SHA-256 of that ISO.
#   --directory PATH   New VM directory, or existing directory with --resume.
#   --memory MB        512-4096, default 1024.
#   --accel NAME       auto (default), kvm or tcg.
#   --network NAME     none (default) or user for outbound package downloads.
#   --headless         Use a local monitor socket instead of a graphical window.
#   --resume           Boot the existing guest disk; do not create or erase it.
#   --options          Print the complete argument list.
#   -h, --help         Print this header without running external tools.
# REQUIREMENTS:
#   Bash, sed, Python 3, qemu-img and qemu-system-i386.
#   KVM acceleration requires access to /dev/kvm; TCG works without it.
#   No physical disks, host folder sharing or incoming network ports are used.
# EXAMPLES:
#   ./vm/qemu.sh --help
#   ./vm/qemu.sh --iso /tmp/antiX.iso --iso-sha256 HEX --directory /tmp/mirror-qemu
#   ./vm/qemu.sh --iso /tmp/antiX.iso --iso-sha256 HEX --directory /tmp/mirror-qemu --resume
# EXIT_CODES:
#   0  Help printed or the guest exited successfully.
#   1  Verification or VM command failed; partial files remain for inspection.
#   2  Unsupported or missing arguments.

set -euo pipefail
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  sed -n '2,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
  exit 0
fi
if [[ $# -eq 1 && "$1" == --options ]]; then
  set -- --help
elif [[ "${1:-}" == -h || "${1:-}" == --help ]]; then
  echo "Unexpected arguments. See ./vm/qemu.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/qemu.py" "$@"
