#!/usr/bin/env bash
# SCRIPT: scripts/appliance/live-install.sh
# DESCRIPTION: Open the official antiX installer only from mounted live media.
# USAGE: ./scripts/appliance/live-install.sh [-h|--help]
# PARAMETERS:
#   No arguments  Launch the official interactive installer from live media.
#   -h, --help    Print this header without launching an application.
# REQUIREMENTS:
#   Root, mounted antiX live media, /usr/bin/minstall and /etc/minstall.conf.
#   Installed systems are refused. Disk choices and confirmations remain interactive.
# EXAMPLES:
#   ./scripts/appliance/live-install.sh --help
#   sudo /usr/local/sbin/specter-mirror-live-install
# EXIT_CODES:
#   0  Help printed or the official installer completed successfully.
#   1  Privilege or live-media checks failed.
#   2  Unsupported arguments.

set -euo pipefail
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  sed -n '2,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
  exit 0
fi
if [[ $# -ne 0 ]]; then
  echo "Unsupported arguments. See --help for usage." >&2
  exit 2
fi
if [[ "$EUID" -ne 0 ]] || ! mountpoint -q /live/boot-dev || [[ ! -f /live/boot-dev/antiX/linuxfs ]]; then
  echo "The installer entry is available only on mounted antiX live media." >&2
  exit 1
fi
if [[ ! -r /etc/minstall.conf ]]; then
  echo "The appliance installer defaults are missing." >&2
  exit 1
fi
exec /usr/bin/minstall --config /etc/minstall.conf
