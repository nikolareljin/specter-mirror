#!/usr/bin/env bash
# SCRIPT: vm/setup-kiosk.sh
# DESCRIPTION: Install or converge the synthetic-proof desktop startup.
# USAGE: ./vm/setup-kiosk.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking prerequisites.
#   --options   Print the complete Python command argument list.
#   --bundle PATH   Verified bundle directory.
#   --sha256 HEX    Trusted SHA-256 of its SHA256SUMS file.
#   --user NAME     Existing non-root antiX desktop account.
#   --profile NAME  auto (default), nb205, d630, virtualbox or generic.
#   --allow-proof   Required opt-in for the synthetic frame application.
#   --offline       Never install packages or access package repositories.
#   --install-deps  Install runtime packages through apt (requires networking).
#   --root PATH     Stage into an absolute fixture root; skip runtime checks.
#   --session NAME  kiosk (default) or desktop. Kiosk bypasses desktop startup.
#   --no-autologin  Keep a login prompt instead of the default kiosk auto-login.
# REQUIREMENTS:
#   Bash, sed, Python 3 and an existing non-root antiX desktop account.
#   System installation requires root; --root staging does not.
# EXAMPLES:
#   ./vm/setup-kiosk.sh --help
#   ./vm/setup-kiosk.sh --options
#   ./vm/setup-kiosk.sh --bundle /media/usb/bundle --sha256 HEX --user mirror --allow-proof
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
  exec python3 "$(dirname "${BASH_SOURCE[0]}")/../scripts/appliance/manage.py" install --help
fi
if [[ "${1:-}" == -h || "${1:-}" == --help ]]; then
  echo "Unexpected arguments. See ./vm/setup-kiosk.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/../scripts/appliance/manage.py" install "$@"

