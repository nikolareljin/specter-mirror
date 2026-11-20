#!/usr/bin/env bash
# SCRIPT: vm/install.sh
# DESCRIPTION: Provision an antiX guest through the shared installer.
# USAGE: ./vm/install.sh [options]
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
#   Bash, sed and Python 3. VM creation needs VBoxManage.
#   ISO creation needs xorriso, a verified base ISO and a verified bundle.
#   Provisioning needs an existing non-root antiX desktop account.
# EXAMPLES:
#   ./vm/install.sh --help
#   ./vm/install.sh --options
#   ./vm/install.sh --bundle /media/usb/bundle --sha256 HEX --user mirror --allow-proof --offline
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
  echo "Unexpected arguments. See ./vm/install.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/../scripts/appliance/manage.py" install "$@"

