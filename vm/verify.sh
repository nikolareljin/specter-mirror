#!/usr/bin/env bash
# SCRIPT: vm/verify.sh
# DESCRIPTION: Verify the guest bundle and startup without claiming a render test.
# USAGE: ./vm/verify.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking prerequisites.
#   --options   Print the complete Python command argument list.
#   --sha256 HEX    Trusted SHA-256 of the installed SHA256SUMS file.
#   --root PATH     Absolute staging root; default is the real system /.
# REQUIREMENTS:
#   Bash, sed and Python 3. VM creation needs VBoxManage.
#   ISO creation needs xorriso, a verified base ISO and a verified bundle.
#   Provisioning needs an existing non-root antiX desktop account.
# EXAMPLES:
#   ./vm/verify.sh --help
#   ./vm/verify.sh --options
#   ./vm/verify.sh --sha256 HEX
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
  exec python3 "$(dirname "${BASH_SOURCE[0]}")/../scripts/appliance/manage.py" verify --help
fi
if [[ "${1:-}" == -h || "${1:-}" == --help ]]; then
  echo "Unexpected arguments. See ./vm/verify.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/../scripts/appliance/manage.py" verify "$@"

