#!/usr/bin/env bash
# SCRIPT: scripts/appliance/install.sh
# DESCRIPTION: Install a verified prebuilt synthetic proof into 32-bit antiX.
# USAGE: ./scripts/appliance/install.sh [options]
# PARAMETERS:
#   -h, --help  Print this header without checking dependencies.
#   --bundle PATH   Verified bundle directory.
#   --sha256 HEX    Trusted SHA-256 of its SHA256SUMS file.
#   --user NAME     Existing non-root antiX desktop account.
#   --profile NAME  auto (default), nb205, d630, virtualbox or generic.
#   --allow-proof   Required opt-in for the synthetic frame application.
#   --offline       Install bundled packages only; never access repositories.
#   --install-deps  Install runtime packages through apt (requires networking).
#   --root PATH     Stage into an absolute fixture root; skip runtime checks.
#   --session NAME  kiosk (default) or desktop. Kiosk bypasses desktop startup.
#   --no-autologin  Keep a login prompt instead of the default kiosk auto-login.
#   --options   Print the detailed command-specific argument list.
# REQUIREMENTS:
#   Bash, sed and Python 3. No Rust or Node toolchain on the target.
#   Install/uninstall require root only for the real system, not --root staging.
# EXAMPLES:
#   ./scripts/appliance/install.sh --help
#   ./scripts/appliance/install.sh --options
#   sudo ./scripts/appliance/install.sh --bundle /media/usb/bundle --sha256 HEX --user mirror --allow-proof --offline
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
  echo "Unexpected arguments. See ./scripts/appliance/install.sh --help for usage." >&2
  exit 2
fi
exec python3 "$(dirname "${BASH_SOURCE[0]}")/manage.py" install "$@"
