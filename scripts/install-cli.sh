#!/usr/bin/env bash
# Source installation of only the `rize` CLI and its embedded read-only service.
# No Tauri/Swift build, GUI bundle, launch agent, or shell config changes.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec cargo install --locked --path src-tauri/cli "$@"
