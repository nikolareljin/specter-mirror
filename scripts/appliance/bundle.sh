#!/usr/bin/env bash
# SCRIPT: scripts/appliance/bundle.sh
# DESCRIPTION: Create a checked prebuilt i686 synthetic-proof bundle.
# USAGE: ./scripts/appliance/bundle.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking dependencies.
#   --binary PATH   Prebuilt ELF32 i686 application executable.
#   --output PATH   New directory to create; existing outputs are refused.
#   --options   Print the detailed command-specific argument list.
# REQUIREMENTS:
#   Bash, sed and Python 3. No Rust or Node toolchain on the target.
#   Install/uninstall require root only for the real system, not --root staging.
# EXAMPLES:
#   ./scripts/appliance/bundle.sh --help
#   ./scripts/appliance/bundle.sh --options
#   ./scripts/appliance/bundle.sh --binary target/i686-unknown-linux-gnu/release/spook-kiosk --output /tmp/mirror-bundle
# EXIT_CODES:
#   0  Help printed or operation completed.
#   1  Validation or provisioning failed.
#   2  Unsupported or incomplete arguments.

set -euo pipefail
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  sed -n '2,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
  exit 0
fi
if [[ $# -eq 1 && "$1" == --options ]]; then
  set -- --help
elif [[ "${1:-}" == -h || "${1:-}" == --help ]]; then
  echo "Unexpected arguments. See ./scripts/appliance/bundle.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/manage.py" bundle "$@"

