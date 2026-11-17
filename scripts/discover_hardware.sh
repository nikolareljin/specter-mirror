#!/usr/bin/env bash
# SCRIPT: discover_hardware.sh
# DESCRIPTION: Collect local compatibility facts for a Linux mirror target.
# USAGE: bash scripts/discover_hardware.sh [-h|--help]
# PARAMETERS:
#   No arguments  Print architecture, OS, CPU, memory, BIOS, PCI, USB, glibc and camera facts.
#   -h, --help    Print this header without probing the machine.
# REQUIREMENTS:
#   Linux and Bash. Optional tools: lscpu, free, lspci, lsusb and v4l2-ctl.
#   Missing tools and failed probes are labelled; remaining probes continue.
#   No sudo, network request or configuration change is performed.
# OUTPUT:
#   Standard output contains local diagnostics and may include device identifiers.
#   Inspect and redact the report before sharing. No report is uploaded.
# EXAMPLES:
#   bash scripts/discover_hardware.sh --help
#   bash scripts/discover_hardware.sh
#   bash scripts/discover_hardware.sh > hardware-local.txt
# EXIT_CODES:
#   0  Help printed or collection finished, possibly with unavailable probes.
#   2  Unsupported arguments.

set -euo pipefail
show_help() {
  sed -n '2,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
}
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  show_help
  exit 0
fi
if [[ $# -ne 0 ]]; then
  echo "Unsupported arguments. See scripts/discover_hardware.sh --help for usage." >&2
  exit 2
fi
run() {
  printf '\n[%s]\n' "$1"
  shift
  if command -v "$1" >/dev/null 2>&1; then
    "$@" 2>&1 || printf 'Command unavailable or failed.\n'
  else
    printf 'Command not installed.\n'
  fi
}
printf 'Local hardware diagnostics. Review and redact before sharing.\n'
run architecture uname -m
run os cat /etc/os-release
run cpu lscpu
run memory free -h
run bios-version cat /sys/class/dmi/id/bios_version
run pci lspci -nn
run usb lsusb
run glibc getconf GNU_LIBC_VERSION
run instruction-flags awk '/^(flags|Features)[[:space:]]*:/ {print; exit}' /proc/cpuinfo
printf '\n[video devices]\n'
shopt -s nullglob
devices=(/dev/video*)
if ((${#devices[@]} == 0)); then
  printf 'No V4L2 device found. Use sample mode or attach a compatible USB webcam.\n'
fi
for device in "${devices[@]}"; do
  printf '%s\n' "$device"
  run camera-formats v4l2-ctl --device "$device" --list-formats-ext
done
