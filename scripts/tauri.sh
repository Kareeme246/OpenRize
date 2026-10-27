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

if [[ "${1:-}" == "build" ]]; then
  shift
  # Updater artifacts must be signed, and only the release workflow holds the
  # key; without it `tauri build` writes the installer and then exits 1. Config
  # files merge in order, so a --config the caller passes after this one wins.
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
    echo "No TAURI_SIGNING_PRIVATE_KEY: building without updater artifacts (source builds don't self-update)." >&2
    exec tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}' "$@"
  fi
  exec tauri build "$@"
fi

exec tauri "$@"
