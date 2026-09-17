#!/usr/bin/env bash
# Single entry point for lint/format/typecheck/clippy. Agents and humans
# should run this (not ad hoc biome/cargo invocations) before calling a task
# done. Pass --fix to apply safe auto-fixes instead of just checking.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

FIX=false
if [[ "${1:-}" == "--fix" ]]; then
  FIX=true
fi

echo "==> biome (frontend lint + format)"
if $FIX; then
  pnpm exec biome check --write .
else
  pnpm exec biome check .
fi

echo "==> tsc (typecheck)"
pnpm exec tsc --noEmit

echo "==> cargo fmt (backend)"
if $FIX; then
  cargo fmt --manifest-path src-tauri/Cargo.toml
else
  cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
fi

echo "==> cargo clippy (backend)"
if $FIX; then
  cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --fix --allow-dirty --allow-staged -- -D warnings
else
  cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
fi

echo "verify: all checks passed"
