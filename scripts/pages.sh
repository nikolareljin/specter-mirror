#!/usr/bin/env bash
# SCRIPT: pages.sh
# DESCRIPTION: Build and verify the browser site before Pages publication.
# USAGE: scripts/pages.sh [build|check|record|tools]
# PARAMETERS:
#   build       Run browser tests and create the production web/dist tree.
#   check       Verify the recorded digests and inspect every output (default).
#   record      Inspect output locally and update web/pages-manifest.json.
#   tools       Install pinned CI tools; set GITHUB_ENV and GITHUB_PATH.
#   -h, --help  Read this header without probing or installing tools.
# REQUIREMENTS:
#   build: npm dependencies, rustup, Rust 1.99.1, wasm32 target and wasm-pack.
#   check/record: Python 3, Git, pinned script-helpers, Gitleaks and Foxguard.
#   SPECTER_PAGES_HELPERS points to the pinned helper checkout outside this repo.
#   record also needs the operator's local private-name dictionary; never run in CI.
#   tools needs a GitHub Actions runner with rustup, cargo, Git and network access.
# OUTPUT:
#   Only web/dist is uploaded. Diagnostics remain outside Git and are not uploaded.
# EXAMPLES:
#   scripts/pages.sh --help
#   scripts/pages.sh build
#   SPECTER_PAGES_HELPERS=/tmp/pages-helpers scripts/pages.sh record
#   SPECTER_PAGES_HELPERS=/tmp/pages-helpers scripts/pages.sh check
# EXIT_CODES:
#   0  Help or the requested operation succeeded.
#   1  Build, tooling or publication verification failed.
#   2  Unsupported arguments.

set -euo pipefail
show_help() {
  sed -n '2,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
}
if [[ $# -eq 1 && ( "$1" == -h || "$1" == --help ) ]]; then
  show_help
  exit 0
fi
[[ $# -le 1 ]] || { echo 'See scripts/pages.sh --help for usage.' >&2; exit 2; }
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
case "${1:-check}" in
  build)
    compiler="$(rustup which --toolchain 1.99.1 rustc)"
    compiler_bin="$(dirname -- "$compiler")"
    export RUSTC="$compiler"
    export PATH="$compiler_bin:${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
    npm --prefix web test
    npm --prefix web run build
    ;;
  check) exec python3 scripts/check_pages.py ;;
  record) exec python3 scripts/check_pages.py --record ;;
  tools)
    [[ "${GITHUB_ACTIONS:-}" == true && -n "${RUNNER_TEMP:-}" && -n "${GITHUB_ENV:-}" && -n "${GITHUB_PATH:-}" ]] || {
      echo 'tools requires a GitHub Actions runner.' >&2; exit 1;
    }
    umask 077
    setup_dir="$(mktemp -d "$RUNNER_TEMP/specter-pages-tools.XXXXXX")"
    setup_tools() {
      helper_dir="$setup_dir/helpers"
      git init -q "$helper_dir"
      git -C "$helper_dir" fetch -q --depth 1 https://github.com/nikolareljin/script-helpers.git 548597c18f70c010a289c29305c45fe84863d12c
      git -C "$helper_dir" checkout -q --detach FETCH_HEAD
      [[ "$(git -C "$helper_dir" rev-parse HEAD)" == 548597c18f70c010a289c29305c45fe84863d12c ]]
      export SCRIPT_HELPERS_DIR="$helper_dir"
      # shellcheck source=/dev/null
      source "$helper_dir/helpers.sh"
      shlib_import foxguard gitleaks
      foxguard_install
      gitleaks_install
      rustup toolchain install 1.99.1 --profile minimal --target wasm32-unknown-unknown
      cargo +1.99.1 install wasm-pack --version 0.15.1 --locked
      printf 'SPECTER_PAGES_HELPERS=%s\n' "$helper_dir" >> "$GITHUB_ENV"
      dirname -- "$(foxguard_cache_path)" >> "$GITHUB_PATH"
      dirname -- "$(gitleaks_cache_path)" >> "$GITHUB_PATH"
    }
    setup_tools > "$setup_dir/setup.log" 2>&1
    ;;
  *) echo 'See scripts/pages.sh --help for usage.' >&2; exit 2 ;;
esac
