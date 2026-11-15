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
  # Updater artifacts must be signed and only the release workflow holds the key; without it `tauri
  # build` exits 1 after writing the installer. Configs merge in order, so a later caller --config
  # wins.
  # `--no-bundle` writes no artifacts, so it gets no override: the release workflow compiles that
  # way without the key, then signs in `tauri bundle`, and the config is compiled into the binary.
  bundling=true
  for arg in "$@"; do
    if [[ "$arg" == "--" ]]; then
      break
    elif [[ "$arg" == "--no-bundle" ]]; then
      bundling=false
    fi
  done
  if $bundling && [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
    echo "No TAURI_SIGNING_PRIVATE_KEY: building without updater artifacts (source builds don't self-update)." >&2
    exec tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}' "$@"
  fi
  exec tauri build "$@"
fi

exec tauri "$@"
