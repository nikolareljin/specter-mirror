#!/usr/bin/env bash
# SCRIPT: vm/create.sh
# DESCRIPTION: Create a new disposable VirtualBox guest without starting it.
# USAGE: ./vm/create.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking prerequisites.
#   --options   Print the complete Python command argument list.
#   --name NAME        New specter-mirror-NAME; existing VMs are refused.
#   --iso PATH         Verified bootable antiX ISO.
#   --iso-sha256 HEX   Trusted checksum of that ISO.
#   --directory PATH  New disposable VM directory.
#   --memory MB       512-4096, default 1024.
#   --disk-mb MB      8192-32768, default 16384.
# REQUIREMENTS:
#   Bash, sed, Python 3 and VBoxManage on the development host.
#   A checksum-verified antiX ISO and a new disposable output directory.
# EXAMPLES:
#   ./vm/create.sh --help
#   ./vm/create.sh --options
#   ./vm/create.sh --name specter-mirror-test --iso /tmp/antiX.iso --iso-sha256 HEX --directory /tmp/mirror-vm
# EXIT_CODES:
#   0  Help printed or the operation completed.
#   1  Validation, compatibility or external command failure.
#   2  Invalid or missing arguments.

set -euo pipefail
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  sed -n '2,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
  exit 0
fi
if [[ $# -eq 1 && "$1" == --options ]]; then
  exec python3 "$(dirname "${BASH_SOURCE[0]}")/virtualbox.py"  --help
fi
if [[ "${1:-}" == -h || "${1:-}" == --help ]]; then
  echo "Unexpected arguments. See ./vm/create.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/virtualbox.py"  "$@"

