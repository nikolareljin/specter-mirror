#!/usr/bin/env bash
# SCRIPT: vm/build-iso.sh
# DESCRIPTION: Build verified antiX appliance or toolkit installation media.
# USAGE: ./vm/build-iso.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking prerequisites.
#   --options   Print the complete Python command argument list.
#   --base PATH           Bootable antiX ISO.
#   --base-sha256 HEX     Trusted checksum from the base distribution.
#   --bundle PATH         Verified proof bundle directory.
#   --bundle-sha256 HEX   Trusted SHA-256 of bundle SHA256SUMS.
#   --output PATH         New ISO path; existing files are refused.
#   --allow-proof         Required opt-in for the display validation binary.
#   --appliance           Embed the app and kiosk setup in the live filesystem.
#   --builder-image NAME  Local isolated builder image for --appliance mode.
# REQUIREMENTS:
#   Bash, sed, Python 3 and xorriso on the build host.
#   A checksum-verified base ISO, verified bundle and space for the output.
#   Appliance mode also requires the isolated Docker builder image.
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
