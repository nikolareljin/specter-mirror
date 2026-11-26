#!/usr/bin/env bash
# SCRIPT: build_wasm.sh
# DESCRIPTION: Build browser WASM with portable dependency source paths.
# USAGE: scripts/build_wasm.sh [-h|--help]
# PARAMETERS:
#   No arguments  Build the face-detection-enabled browser bindings.
#   -h, --help    Print this header without probing tools or building.
# REQUIREMENTS:
#   Bash, wasm-pack and the repository's Rust toolchain with the wasm32 target.
#   Existing Cargo flags are preserved. Source paths use stable build prefixes.
# OUTPUT:
#   Generated WASM and JavaScript bindings in web/generated.
# EXAMPLES:
#   scripts/build_wasm.sh --help
#   scripts/build_wasm.sh
#   npm --prefix web run wasm:build
# EXIT_CODES:
#   0  Help printed or WASM built successfully.
#   2  Unsupported arguments.
#   Other nonzero values are propagated from the build tools.

set -euo pipefail
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  sed -n '2,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
  exit 0
fi
[[ $# -eq 0 ]] || { echo 'See scripts/build_wasm.sh --help for usage.' >&2; exit 2; }
mirror_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
mirror_flags="${CARGO_ENCODED_RUSTFLAGS:-}"
if [[ "${CARGO_ENCODED_RUSTFLAGS+x}" != x && -n "${RUSTFLAGS:-}" ]]; then
  read -r -a mirror_existing_flags <<< "${RUSTFLAGS//$'\n'/ }"
  for mirror_flag in "${mirror_existing_flags[@]}"; do
    mirror_flags+="${mirror_flags:+$'\x1f'}$mirror_flag"
  done
fi
for mirror_mapping in "$HOME=/build/home" "${CARGO_HOME:-$HOME/.cargo}=/build/cargo" "$mirror_root=/build/specter-mirror"; do
  mirror_flags+="${mirror_flags:+$'\x1f'}--remap-path-prefix=$mirror_mapping"
done
export CARGO_ENCODED_RUSTFLAGS="$mirror_flags"
cd -- "$mirror_root/web"
exec wasm-pack build ../crates/spook-web --target web --out-dir ../../web/generated --no-opt -- --features face-detection
