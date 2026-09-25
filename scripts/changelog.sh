#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
git-cliff --config cliff.toml --output CHANGELOG.md
