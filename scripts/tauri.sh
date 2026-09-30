#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-}" == "dev" ]]; then
  shift
  for arg in "$@"; do
    if [[ "$arg" == "-c" || "$arg" == "--config" || "$arg" == --config=* ]]; then
      exec tauri dev "$@"
    fi
  done
  exec tauri dev "$@" --config '{"identifier":"com.offlinestudios.openrize.dev"}'
fi

exec tauri "$@"
