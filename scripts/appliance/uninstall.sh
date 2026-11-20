#!/usr/bin/env bash
# SCRIPT: scripts/appliance/uninstall.sh
# DESCRIPTION: Remove verified managed files while preserving desktop user data.
# USAGE: ./scripts/appliance/uninstall.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking dependencies.
#   --sha256 HEX    Trusted SHA-256 of the installed SHA256SUMS file.
#   --root PATH     Absolute staging root; default is the real system /.
#   --options   Print the detailed command-specific argument list.
# REQUIREMENTS:
#   Bash, sed and Python 3. No Rust or Node toolchain on the target.
#   Install/uninstall require root only for the real system, not --root staging.
# EXAMPLES:
#   ./scripts/appliance/uninstall.sh --help
#   ./scripts/appliance/uninstall.sh --options
#   sudo ./scripts/appliance/uninstall.sh --sha256 HEX
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
  echo "Unexpected arguments. See ./scripts/appliance/uninstall.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/manage.py" uninstall "$@"

