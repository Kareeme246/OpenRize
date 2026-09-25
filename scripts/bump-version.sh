#!/usr/bin/env bash
# Bumps the version in package.json, src-tauri/tauri.conf.json, and
# src-tauri/Cargo.toml together, so the three can never drift. See
# RELEASING.md for the full release procedure.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ $# -ne 1 ]]; then
  echo "usage: $0 X.Y.Z" >&2
  exit 1
fi

VERSION="$1"
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: version must be plain semver X.Y.Z (got: $VERSION)" >&2
  exit 1
fi

set_json_version() {
  local file="$1"
  # Only the top-level "version" field (the first match), so nested
  # dependency version strings elsewhere in the file are left alone.
  perl -0pi -e 's/("version":\s*")[^"]*(")/${1}'"$VERSION"'${2}/' "$file"
}

set_json_version package.json
set_json_version src-tauri/tauri.conf.json

perl -0pi -e 's/(^version\s*=\s*")[^"]*(")/${1}'"$VERSION"'${2}/m' src-tauri/Cargo.toml

echo "bumped package.json, src-tauri/tauri.conf.json, and src-tauri/Cargo.toml to $VERSION"
