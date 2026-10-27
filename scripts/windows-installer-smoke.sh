#!/usr/bin/env bash
# Installs the built NSIS installer on this Windows machine, as a user would,
# and checks what its hooks promise: the app and rize land in the install
# folder, rize is on the user PATH (once, even after an in-app style update)
# and the uninstaller takes the PATH entry and the login item away again.
#
# The Release workflow's windows job runs this after `pnpm tauri build
# --bundles nsis`, on a disposable runner whose registry is ours to use. It is
# a script rather than inline YAML so the same gate can be run on its own.
#
# Every program it starts is bounded: a hang must name the program that hung
# and fail in seconds, never sit in the job timeout with no output. The
# arguments each program gets are written into a generated PowerShell file
# rather than passed as arguments or environment values, because Git Bash
# rewrites anything that looks like a POSIX path - it turns the installer's
# silent switch `/S` into `S:/`, which starts a wizard nobody can click.
set -euo pipefail

VERSION="${VERSION:?VERSION (the version being built) must be set}"

# Long enough for a slow runner, short enough that a hang is obvious.
RUN_LIMIT=120
INSTALL_LIMIT=240

TMP_POSIX=$(mktemp -d)
TMP=$(cygpath -w "$TMP_POSIX")
RUNS=0
trap 'rm -rf "$TMP_POSIX"' EXIT

fail() {
  echo "::error title=Windows installer smoke test::$*"
  exit 1
}
# A progress marker before each action, so the log says where a hang happened.
step() {
  printf -- '--- [%s] %s\n' "$(date -u +%H:%M:%SZ)" "$*"
}
# A PowerShell single-quoted string literal.
ps_string() {
  printf "'%s'" "${1//\'/\'\'}"
}

case "$(uname -s)" in
MSYS_NT* | MINGW*) ;;
*) fail "this smoke test installs a Windows installer; run it on Windows" ;;
esac

# run_bounded <windows exe> <arguments> <label> [limit]
#
# Starts the program, waits at most LIMIT seconds for it, and leaves its stdout
# in RESPONSE_OUT and its status in RESPONSE_STATUS. On a timeout it prints
# every process in the started program's tree - so the log names the culprit
# instead of going quiet - kills that tree, and returns 124.
run_bounded() {
  local exe=$1 args=${2-} label=$3 limit=${4-$RUN_LIMIT}
  local script="$TMP_POSIX/run$RUNS.ps1"
  RUNS=$((RUNS + 1))
  RESPONSE_OUT="$TMP\\run$RUNS.out"
  step "$label: $exe $args (bound ${limit}s)"
  {
    printf '$exe = %s\n' "$(ps_string "$exe")"
    printf '$arguments = %s\n' "$(ps_string "$args")"
    printf '$label = %s\n' "$(ps_string "$label")"
    printf '$limit = %d\n' "$limit"
    printf '$log = %s\n' "$(ps_string "$RESPONSE_OUT")"
    cat <<'POWERSHELL'
$ErrorActionPreference = 'Stop'
$started = Start-Process -FilePath $exe -ArgumentList $arguments -PassThru `
  -RedirectStandardOutput $log -RedirectStandardError "$log.err"
if (-not $started.WaitForExit($limit * 1000)) {
  $processes = Get-CimInstance Win32_Process
  $tree = [System.Collections.Generic.List[int]]::new()
  $tree.Add($started.Id)
  $grew = $true
  while ($grew) {
    $grew = $false
    foreach ($candidate in $processes) {
      if ($tree.Contains($candidate.ParentProcessId) -and
        -not $tree.Contains($candidate.ProcessId)) {
        $tree.Add($candidate.ProcessId)
        $grew = $true
      }
    }
  }
  $hung = @()
  foreach ($processId in $tree) {
    $found = $processes | Where-Object { $_.ProcessId -eq $processId }
    if ($found) {
      $hung += "$($found.Name) (pid $processId)"
      Write-Host "    still running: $($found.Name) (pid $processId, " `
        "parent $($found.ParentProcessId)): $($found.CommandLine)"
    }
  }
  $names = $hung -join ', '
  Write-Host "::error title=Windows installer smoke test::$label did not " `
    "finish within ${limit}s; still running: $names"
  foreach ($processId in $tree) {
    Stop-Process -Id $processId -Force -ErrorAction SilentlyContinue
  }
  exit 124
}
exit $started.ExitCode
POWERSHELL
  } >"$script"
  set +e
  MSYS_NO_PATHCONV=1 powershell.exe -NoProfile -NonInteractive \
    -ExecutionPolicy Bypass -File "$(cygpath -w "$script")"
  RESPONSE_STATUS=$?
  set -e
  if [[ $RESPONSE_STATUS -ne 0 && -s "$(cygpath -u "$RESPONSE_OUT").err" ]]; then
    sed 's/^/    stderr: /' <"$(cygpath -u "$RESPONSE_OUT").err"
  fi
  return $RESPONSE_STATUS
}

user_path() {
  MSYS_NO_PATHCONV=1 powershell.exe -NoProfile -NonInteractive -Command \
    "[Environment]::GetEnvironmentVariable('Path', 'User')" | tr -d '\r'
}
# How many PATH entries are the folder, ignoring case and a trailing backslash.
path_entries() {
  user_path | tr ';' '\n' | sed 's/[\]$//' | grep -Fxic "$1" || true
}

installer=$(cygpath -w "$PWD/src-tauri/target/release/bundle/nsis/OpenRize_${VERSION}_x64-setup.exe")
[[ -f "$(cygpath -u "$installer")" ]] || fail "installer not found: $installer"

dir_win="$LOCALAPPDATA\OpenRize"
dir=$(cygpath -u "$dir_win")
run_key='HKCU\Software\Microsoft\Windows\CurrentVersion\Run'

step "installer: $installer"
step "install folder: $dir_win"

run_bounded "$installer" "/S" "silent install" "$INSTALL_LIMIT" ||
  fail "silent install exited with $RESPONSE_STATUS"
[[ -f "$dir/openrize.exe" ]] || fail "openrize.exe is missing from $dir_win"
[[ -f "$dir/rize.exe" ]] || fail "rize.exe is missing from $dir_win"
[[ -f "$dir/uninstall.exe" ]] || fail "uninstall.exe is missing from $dir_win"

run_bounded "$dir_win\\rize.exe" "--version" "rize --version" ||
  fail "rize --version exited with $RESPONSE_STATUS"
reported=$(tr -d '\r' <"$(cygpath -u "$RESPONSE_OUT")")
[[ "$reported" == "rize $VERSION" ]] ||
  fail "rize --version printed '$reported', expected 'rize $VERSION'"

step "checking the user PATH"
[[ $(path_entries "$dir_win") == 1 ]] ||
  fail "$dir_win is not on the user PATH exactly once after install"

# An in-app update runs the installer again over the install, with /UPDATE.
run_bounded "$installer" "/S /UPDATE" "silent update" "$INSTALL_LIMIT" ||
  fail "silent update exited with $RESPONSE_STATUS"
[[ -f "$dir/rize.exe" ]] || fail "rize.exe is missing after the update"
step "checking the user PATH again"
[[ $(path_entries "$dir_win") == 1 ]] ||
  fail "$dir_win is not on the user PATH exactly once after an update"

# Stand-in for the login item the app writes, to prove uninstall clears it.
step "registering the login item stand-in"
MSYS_NO_PATHCONV=1 reg add "$run_key" /v OpenRize /t REG_SZ \
  /d "\"$dir_win\openrize.exe\" --background" /f >/dev/null

# _?= keeps the uninstaller in place so that this call waits for it.
run_bounded "$dir_win\\uninstall.exe" "/S _?=$dir_win" "silent uninstall" \
  "$INSTALL_LIMIT" || fail "silent uninstall exited with $RESPONSE_STATUS"
[[ ! -e "$dir/openrize.exe" && ! -e "$dir/rize.exe" ]] ||
  fail "the uninstaller left the app or rize.exe in $dir_win"
step "checking the user PATH after uninstall"
[[ $(path_entries "$dir_win") == 0 ]] ||
  fail "$dir_win is still on the user PATH after uninstall"
if MSYS_NO_PATHCONV=1 reg query "$run_key" /v OpenRize >/dev/null 2>&1; then
  fail "the OpenRize login item is still registered after uninstall"
fi

step "done"
echo "Windows installer install, update and uninstall checks passed."
