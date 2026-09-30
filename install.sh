#!/usr/bin/env bash
# Installs the prebuilt, read-only CLI. No Rust, GUI, sudo, or shell edits.
set -euo pipefail
umask 077

fail() {
  printf 'openrize installer: %s\n' "$1" >&2
  exit 1
}

usage() {
  printf '%s\n' \
    'Install the standalone OpenRize CLI (Apple Silicon macOS).' \
    'Usage: bash install.sh [--dir DIRECTORY] [--version X.Y.Z]' \
    'Default: latest release, ~/.local/bin; existing files are never replaced.' \
    'Downloads are checked against SHA-256 and a notarized OpenRize Developer ID signature.'
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
for tool in curl unzip shasum codesign; do
  command -v "$tool" >/dev/null 2>&1 || fail "Required command is missing: $tool"
done

base='https://github.com/Kareeme246/OpenRize/releases'
if [[ "$version" == latest ]]; then base="$base/latest/download"; else base="$base/download/v$version"; fi
asset='openrize-cli-darwin-aarch64.zip'
work=$(mktemp -d "${TMPDIR:-/tmp}/openrize-install.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Constrain redirects to HTTPS, bound the wait, and never execute a download
# until the archive hash, executable signature, and notarization are checked.
curl_args=(--fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 10 --max-time 120)
curl "${curl_args[@]}" "$base/$asset" --output "$work/$asset" || \
  fail 'Could not download the CLI. Standalone releases start at v0.8.4.'
curl "${curl_args[@]}" "$base/$asset.sha256" --output "$work/checksum" || \
  fail 'Could not download the release checksum; nothing was installed.'
read -r expected filename < "$work/checksum" || fail 'Invalid release checksum.'
[[ "$expected" =~ ^[0-9a-f]{64}$ && "$filename" == "$asset" ]] || fail 'Invalid release checksum.'
actual=$(shasum -a 256 "$work/$asset")
[[ "${actual%% *}" == "$expected" ]] || fail 'Checksum mismatch; nothing was installed.'

# Extract only the two known files to chosen paths, never archive-supplied paths.
unzip -p "$work/$asset" openrize-cli/openrize > "$work/openrize" || fail 'Invalid CLI archive.'
unzip -p "$work/$asset" openrize-cli/LICENSE > "$work/LICENSE" || fail 'Missing license in CLI archive.'
[[ -s "$work/openrize" && -s "$work/LICENSE" ]] || fail 'Empty CLI archive.'
chmod 755 "$work/openrize"
# Gatekeeper's spctl rejects every bare command-line tool, so require Apple's
# notarization and the OpenRize Developer ID team directly.
requirement='notarized and anchor apple generic and certificate leaf[subject.OU] = "Z899WY5Y94"'
codesign --verify --strict --check-notarization -R="$requirement" "$work/openrize" || \
  fail 'CLI is not notarized and signed by OpenRize; nothing was installed.'

destination="$install_dir/openrize"
license="$install_dir/openrize.LICENSE"
if [[ -e "$destination" || -L "$destination" || -e "$license" || -L "$license" ]]; then
  if [[ -f "$destination" && ! -L "$destination" && -f "$license" && ! -L "$license" ]] && \
    cmp -s "$work/openrize" "$destination" && cmp -s "$work/LICENSE" "$license"; then
    printf 'OpenRize CLI is already installed at %s\n' "$destination"
    exit 0
  fi
  fail "An installation path already exists in $install_dir; nothing was replaced. Remove the old CLI and its license explicitly to reinstall."
fi

mkdir -p "$install_dir"
# Bash noclobber creates files exclusively, including during racing installs.
# Files remain non-executable until both copies finish; a failed copy rolls back
# only files created by this invocation, never an existing executable/symlink.
(
  set -o noclobber
  exec 3> "$destination"
  created_license=false
  trap 'rm -f "$destination"; if $created_license; then rm -f "$license"; fi' EXIT
  exec 4> "$license"
  created_license=true
  cat "$work/openrize" >&3
  cat "$work/LICENSE" >&4
  chmod 644 "$license"
  chmod 755 "$destination"
  trap - EXIT
)
printf 'Installed OpenRize CLI at %s\n' "$destination"
printf 'No shell configuration was changed. If needed, add %s to PATH.\n' "$install_dir"
printf 'Next: openrize --json status\n'
