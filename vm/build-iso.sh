#!/usr/bin/env bash
# SCRIPT: vm/build-iso.sh
# DESCRIPTION: Create an antiX toolkit preview ISO, preserving the live filesystem.
# USAGE: ./vm/build-iso.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking prerequisites.
#   --options   Print the complete Python command argument list.
#   --base PATH           Bootable antiX ISO.
#   --base-sha256 HEX     Trusted checksum from the base distribution.
#   --bundle PATH         Verified proof bundle directory.
#   --bundle-sha256 HEX   Trusted SHA-256 of bundle SHA256SUMS.
#   --output PATH         New ISO path; existing files are refused.
#   --allow-proof         Required opt-in; this is toolkit preview media.
# REQUIREMENTS:
#   Bash, sed and Python 3. VM creation needs VBoxManage.
#   ISO creation needs xorriso, a verified base ISO and a verified bundle.
#   Provisioning needs an existing non-root antiX desktop account.
# EXAMPLES:
#   ./vm/build-iso.sh --help
#   ./vm/build-iso.sh --options
#   ./vm/build-iso.sh --base /tmp/antiX.iso --base-sha256 HEX --bundle /tmp/bundle --bundle-sha256 HEX --output /tmp/mirror-preview.iso --allow-proof
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
  exec python3 "$(dirname "${BASH_SOURCE[0]}")/iso.py"  --help
fi
if [[ "${1:-}" == -h || "${1:-}" == --help ]]; then
  echo "Unexpected arguments. See ./vm/build-iso.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/iso.py"  "$@"

