#!/usr/bin/env bash
# Installs the notarized `rize` CLI from the matching OpenRize app release.
set -euo pipefail
umask 077

fail() { printf 'rize installer: %s\n' "$1" >&2; exit 1; }
usage() {
  printf '%s\n' \
    'Install the rize CLI for OpenRize (Apple Silicon macOS).' \
    'Usage: bash install.sh [--dir DIRECTORY] [--version X.Y.Z] [--yes] [--uninstall]' \
    'Default: latest release, ~/.local/bin. --yes confirms an update without prompting.' \
    'The CLI must be notarized and signed by the OpenRize Developer ID.'
}
install_dir=''
version=latest
yes=0
uninstall=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --dir|--version)
      [[ $# -ge 2 && -n "$2" && "$2" != --* ]] || { usage >&2; exit 2; }
      if [[ "$1" == --dir ]]; then install_dir="$2"; else version="$2"; fi
      shift 2;;
    --yes|-y) yes=1; shift;;
    --uninstall) uninstall=1; shift;;
    -h|--help) usage; exit 0;;
    *) usage >&2; printf 'Unknown argument: %s\n' "$1" >&2; exit 2;;
  esac
done
[[ "$version" == latest || "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { printf 'Version must be X.Y.Z or latest.\n' >&2; exit 2; }
[[ -n "$install_dir" || -n "${HOME:-}" ]] || fail 'HOME is unset; pass --dir explicitly.'
[[ -n "$install_dir" ]] || install_dir="$HOME/.local/bin"
[[ "$install_dir" == /* ]] || install_dir="$PWD/$install_dir"
destination="$install_dir/rize"
if (( uninstall )); then
  if [[ -e "$destination" || -L "$destination" ]]; then rm -f "$destination" || fail "Could not remove $destination"; printf 'Removed %s\n' "$destination"
  else printf 'No installed rize at %s\n' "$destination"; fi
  exit 0
fi
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || fail 'Prebuilt releases currently support Apple Silicon macOS only.'
for tool in curl tar codesign; do command -v "$tool" >/dev/null 2>&1 || fail "Required command is missing: $tool"; done
base='https://github.com/Kareeme246/OpenRize/releases'
if [[ "$version" == latest ]]; then base="$base/latest/download"; else base="$base/download/v$version"; fi
asset='openrize.app.tar.gz'
member='openrize.app/Contents/MacOS/rize'
work=$(mktemp -d "${TMPDIR:-/tmp}/rize-install.XXXXXX")
trap 'rm -rf "$work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
curl_args=(--fail --silent --show-error --location --proto '=https' --proto-redir '=https' --tlsv1.2 --connect-timeout 10 --max-time 300)
curl "${curl_args[@]}" "$base/$asset" --output "$work/$asset" || fail 'Could not download the OpenRize release.'
tar -xzOf "$work/$asset" "$member" > "$work/rize" 2>/dev/null && [[ -s "$work/rize" ]] || fail 'This release does not include the CLI.'
chmod 755 "$work/rize"
requirement='notarized and anchor apple generic and certificate leaf[subject.OU] = "Z899WY5Y94"'
codesign --verify --strict --check-notarization -R="$requirement" "$work/rize" || fail 'CLI is not notarized and signed by OpenRize; nothing was installed.'
new_version=$(/bin/bash -c '"$1" --version' _ "$work/rize" | awk '{print $NF}')
[[ "$new_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail 'Could not determine the downloaded CLI version.'
old_version=''
if [[ -e "$destination" || -L "$destination" ]]; then
  if [[ -f "$destination" && ! -L "$destination" ]] && cmp -s "$work/rize" "$destination"; then
    printf 'rize is already installed at %s\n' "$destination"; exit 0
  fi
  if [[ -x "$destination" ]]; then
    old_version=$("$destination" --version 2>/dev/null | awk '{print $NF}' || true)
  fi
  [[ "$old_version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "$destination exists but its rize version cannot be determined; nothing was replaced. Remove it explicitly to continue."
  # Compare numeric components without relying on GNU-only sort -V.
  older=$(awk -v a="$old_version" -v b="$new_version" 'BEGIN { split(a,x,"."); split(b,y,"."); for(i=1;i<=3;i++){if(x[i]+0<y[i]+0)exit 0;if(x[i]+0>y[i]+0)exit 1} exit 1 }' && echo yes || echo no)
  [[ "$older" == yes ]] || { printf 'Installed rize %s is not older than release %s; leaving it unchanged.\n' "$old_version" "$new_version"; exit 0; }
  if (( ! yes )); then
    printf 'Update from %s to %s? [y/N] ' "$old_version" "$new_version" >/dev/tty 2>/dev/null || fail 'No interactive terminal. Re-run with --yes to confirm the update.'
    IFS= read -r answer </dev/tty || fail 'Could not read confirmation from terminal.'
    [[ "$answer" == y || "$answer" == Y || "$answer" == yes || "$answer" == YES ]] || { printf 'Update cancelled.\n'; exit 0; }
  fi
fi
mkdir -p "$install_dir"
temporary="$install_dir/.rize-install-$$"
cp "$work/rize" "$temporary" || { rm -f "$temporary"; fail 'Could not stage rize.'; }
chmod 755 "$temporary"
mv -f "$temporary" "$destination"
printf 'Installed rize %s at %s\n' "$new_version" "$destination"
case ":${PATH:-}:" in *":$install_dir:"*) :;; *) printf 'Add %s to PATH to run rize from a terminal, for example:\n  export PATH="%s:$PATH"\n' "$install_dir" "$install_dir";; esac
printf 'No shell configuration was changed. Next: rize --json status\n'
