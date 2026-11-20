#!/usr/bin/env bash
# SCRIPT: scripts/appliance/maintenance.sh
# DESCRIPTION: Enable, disable or inspect kiosk startup as the desktop user.
# USAGE: ./scripts/appliance/maintenance.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking prerequisites.
#   --options   Print the complete Python command argument list.
#   status   Show whether the desktop user's kiosk is enabled.
#   disable  Request exit and suppress startup at future logins.
#   enable   Permit startup at the next desktop login.
# REQUIREMENTS:
#   Bash, sed and Python 3. VM creation needs VBoxManage.
#   ISO creation needs xorriso, a verified base ISO and a verified bundle.
#   Provisioning needs an existing non-root antiX desktop account.
# EXAMPLES:
#   ./scripts/appliance/maintenance.sh --help
#   ./scripts/appliance/maintenance.sh --options
#   ./scripts/appliance/maintenance.sh disable
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
  exec python3 "$(dirname "${BASH_SOURCE[0]}")/run.py"  --help
fi
if [[ "${1:-}" == -h || "${1:-}" == --help ]]; then
  echo "Unexpected arguments. See ./scripts/appliance/maintenance.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/run.py" --maintenance "$@"

