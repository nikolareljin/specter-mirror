#!/usr/bin/env bash
set -u
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
