#!/usr/bin/env bash
# Puts `rize` on PATH by linking to the copy inside the installed OpenRize app,
# so rize always matches the app and updates with it. Downloads nothing; no
# sudo or shell edits.
set -euo pipefail

identifier='com.offlinestudios.openrize'
no_app="No rize tracking information found. Are you sure you've installed the app?"

fail() {
  printf 'rize installer: %s\n' "$1" >&2
  exit 1
}

usage() {
  printf '%s\n' \
    'Link the rize CLI from the installed OpenRize app (macOS).' \
    'Usage: bash install.sh [--dir DIRECTORY] [--app PATH/TO/openrize.app]' \
    'Default: ~/.local/bin; existing files are never replaced.' \
    'The linked rize must be notarized and signed by the OpenRize Developer ID.'
}

install_dir=''
app=''
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir|--app)
      [[ $# -ge 2 && -n "$2" && "$2" != --* ]] || { usage >&2; exit 2; }
      if [[ "$1" == --dir ]]; then install_dir="$2"; else app="$2"; fi
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done

[[ "$(uname -s)" == Darwin ]] || \
  fail 'This installer is for macOS. On Windows, run `rize install-path` from your build.'
if [[ -z "$install_dir" ]]; then
  [[ -n "${HOME:-}" ]] || fail 'HOME is unset; pass --dir explicitly.'
  install_dir="$HOME/.local/bin"
fi
[[ "$install_dir" == /* ]] || install_dir="$PWD/$install_dir"

# The usual install locations, then wherever Spotlight knows the app to be.
if [[ -z "$app" ]]; then
  for candidate in /Applications/openrize.app "${HOME:-}/Applications/openrize.app"; do
    if [[ -d "$candidate" ]]; then app="$candidate"; break; fi
  done
fi
if [[ -z "$app" ]] && command -v mdfind >/dev/null 2>&1; then
  app=$(mdfind "kMDItemCFBundleIdentifier == '$identifier'" 2>/dev/null | head -n 1 || true)
fi
[[ -n "$app" && -d "$app" ]] || fail "$no_app"
app=$(cd "$app" && pwd -P)
cli="$app/Contents/MacOS/rize"
[[ -f "$cli" && -x "$cli" ]] || \
  fail "$app does not include rize yet. Update OpenRize; rize ships with it from v0.8.10."

# Gatekeeper's spctl rejects every bare command-line tool, so require Apple's
# notarization and the OpenRize Developer ID team directly. Keep the team in
# sync with RELEASING.md.
requirement='notarized and anchor apple generic and certificate leaf[subject.OU] = "Z899WY5Y94"'
codesign --verify --strict --check-notarization -R="$requirement" "$cli" || \
  fail "$cli is not notarized and signed by OpenRize; nothing was linked. For a build of your own, run its \`rize install-path\`."

destination="$install_dir/rize"
if [[ -L "$destination" && "$(readlink "$destination")" == "$cli" ]]; then
  printf 'rize is already installed at %s\n' "$destination"
  exit 0
fi
if [[ -e "$destination" || -L "$destination" ]]; then
  fail "$destination already exists; nothing was replaced. Remove it explicitly to reinstall."
fi
mkdir -p "$install_dir"
# `ln -s` without -f refuses an existing path, so a racing install never
# replaces what another one just created.
ln -s "$cli" "$destination" 2>/dev/null || {
  [[ -L "$destination" && "$(readlink "$destination")" == "$cli" ]] || \
    fail "$destination already exists; nothing was replaced."
}
printf 'Installed rize at %s, linked to %s\n' "$destination" "$cli"
case ":${PATH:-}:" in
  *":$install_dir:"*) ;;
  *) printf 'No shell configuration was changed; add %s to PATH.\n' "$install_dir" ;;
esac
printf 'Next: rize status\n'
