#!/usr/bin/env bash
# Installs the `rize` CLI on its own, taken from a release's notarized app
# bundle. No Rust, GUI install, sudo, or shell edits. rize needs the app's data
# to do anything; the app also bundles its own copy.
set -euo pipefail
umask 077

fail() {
  printf 'rize installer: %s\n' "$1" >&2
  exit 1
}

usage() {
  printf '%s\n' \
    'Install the rize CLI for OpenRize (Apple Silicon macOS).' \
    'Usage: bash install.sh [--dir DIRECTORY] [--version X.Y.Z]' \
    'Default: latest release, ~/.local/bin; existing files are never replaced.' \
    'The CLI must be notarized and signed by the OpenRize Developer ID.'
}

install_dir=''
version=latest
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir|--version)
      [[ $# -ge 2 && -n "$2" && "$2" != --* ]] || { usage >&2; exit 2; }
      if [[ "$1" == --dir ]]; then install_dir="$2"; else version="$2"; fi
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) usage >&2; printf 'Unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done

if [[ "$version" != latest && ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  printf 'Version must be X.Y.Z or latest.\n' >&2
  exit 2
fi
if [[ -z "$install_dir" ]]; then
  [[ -n "${HOME:-}" ]] || fail 'HOME is unset; pass --dir explicitly.'
  install_dir="$HOME/.local/bin"
fi
[[ "$install_dir" == /* ]] || install_dir="$PWD/$install_dir"
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || \
  fail 'Prebuilt releases currently support Apple Silicon macOS only.'
for tool in curl tar codesign; do
  command -v "$tool" >/dev/null 2>&1 || fail "Required command is missing: $tool"
done

base='https://github.com/Kareeme246/OpenRize/releases'
if [[ "$version" == latest ]]; then base="$base/latest/download"; else base="$base/download/v$version"; fi
# The CLI ships inside the app bundle, so it reuses the updater's archive.
asset='openrize.app.tar.gz'
member='openrize.app/Contents/MacOS/rize'
work=$(mktemp -d "${TMPDIR:-/tmp}/rize-install.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Constrain redirects to HTTPS, bound the wait, and never execute a download
# until its signature and notarization are checked.
curl_args=(--fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 10 --max-time 300)
curl "${curl_args[@]}" "$base/$asset" --output "$work/$asset" || \
  fail 'Could not download the OpenRize release.'
# Extract only the one known file to a chosen path, never archive-supplied paths.
tar -xzOf "$work/$asset" "$member" > "$work/rize" 2>/dev/null && [[ -s "$work/rize" ]] || \
  fail 'This release does not include the CLI; it ships from v0.8.10.'
chmod 755 "$work/rize"
# Gatekeeper's spctl rejects every bare command-line tool, so require Apple's
# notarization and the OpenRize Developer ID team directly. Keep the team in
# sync with RELEASING.md.
requirement='notarized and anchor apple generic and certificate leaf[subject.OU] = "Z899WY5Y94"'
codesign --verify --strict --check-notarization -R="$requirement" "$work/rize" || \
  fail 'CLI is not notarized and signed by OpenRize; nothing was installed.'

destination="$install_dir/rize"
if [[ -e "$destination" || -L "$destination" ]]; then
  if [[ -f "$destination" && ! -L "$destination" ]] && cmp -s "$work/rize" "$destination"; then
    printf 'rize is already installed at %s\n' "$destination"
    exit 0
  fi
  fail "$destination already exists; nothing was replaced. Remove it explicitly to reinstall."
fi

mkdir -p "$install_dir"
# Bash noclobber creates the file exclusively, including during racing
# installs. It stays non-executable until the copy finishes, and a failed copy
# removes only the file this invocation created.
(
  set -o noclobber
  exec 3> "$destination"
  trap 'rm -f "$destination"' EXIT
  cat "$work/rize" >&3
  chmod 755 "$destination"
  trap - EXIT
)
printf 'Installed rize at %s\n' "$destination"
printf 'No shell configuration was changed. If needed, add %s to PATH.\n' "$install_dir"
printf 'Next: rize --json status\n'
