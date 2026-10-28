#!/usr/bin/env bash
# Launch an isolated OpenRize instance, click through its real webview UI, read
# the result back, and tear the instance down. macOS only.
#
# This is the interactive sibling of dev-screenshot.sh: same isolation recipe
# (own free port, own Tauri `identifier`, own config dir, own process group) but
# the instance stays up between commands so an agent can actually drive it.
#
# Every session is namespaced by the checkout it was started from, so two agents
# in two worktrees never see or kill each other's sessions.
#
# See .agents/skills/drive-app/SKILL.md for the workflow.
set -euo pipefail
set -m

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$REPO_ROOT"

WORKSPACE_KEY="$(printf '%s' "$REPO_ROOT" | shasum 2>/dev/null | cut -c1-10 || true)"
SESSION_ROOT="${TMPDIR:-/tmp}/openrize-app-drive/$WORKSPACE_KEY"
AX_DRIVE_JS="$REPO_ROOT/scripts/ax-drive.js"

# How long to wait for the app binary to appear. A warm target/ dir gets there in
# seconds; a cold `cargo build` of the whole dependency tree can take minutes.
STARTUP_TIMEOUT="${OPENRIZE_DRIVE_TIMEOUT:-300}"

usage() {
  cat <<'USAGE'
usage: scripts/app-drive.sh <command> [args]

  doctor [--json]             check this machine can launch and drive an instance (launches nothing)
  start  [--json]             launch an isolated instance, print its session id
  list   [--json]             list live sessions started from this checkout
  tree   [-s ID] [--full]     print the window's accessibility tree
  find   [-s ID] <name>       resolve a control by its accessible name
  click  [-s ID] <name>       click a control (real DOM click via AXPress)
  fill   [-s ID] <name> <txt> focus a field and type text into it
  key    [-s ID] <keyspec>    send a keystroke, e.g. return, tab, backspace, delete, cmd+comma
  wait   [-s ID] <text> [sec] block until text appears in the tree (default 15s)
  shot   [-s ID] [--out PATH] screenshot the window, print the PNG path
  logs   [-s ID] [n]          tail the instance's dev-server/cargo log
  rize   [-s ID] -- <args>    run the repo's rize CLI against this session's data
  stop   [-s ID | --all]      kill the instance and delete its data

Options common to the driving commands:
  -s, --session ID   target a specific session (default: the only live one)
  -r, --role ROLE    restrict matching to an AX role, e.g. AXRadioButton
  -i, --index N      pick the Nth match when a name is ambiguous

Other options:
  --json             doctor, start, list, stop: print one JSON object on stdout
  --dry-run          stop: kill and delete nothing, print what would happen
  --out PATH         shot: write the PNG to PATH (must end in .png) instead of
                     the session's shots dir; PATH survives 'stop'
  --all              stop: every session from this checkout, including stale ones
                     (their process already died but their directories remain)

rize runs the repo's own CLI with --data-dir set to the session's data, so you can
seed data and read backend state through the same tool. Everything after the first
'--' goes to rize verbatim (--json, --full, --yes, --dry-run reach rize). The
instance holds its stores, so rize reaches it over the app's local endpoint and
changes show in the window at once. RIZE_APP points at a path that does not exist,
so 'rize app start' or 'rize track start' can never launch a second app outside
the session. The first call compiles the CLI crate (debug profile); later ones are fast.

'stop' without --dry-run moves a session's screenshots (shots made without --out)
to ${TMPDIR:-/tmp}/openrize-app-drive-evidence/<session id>/ before deleting the
session, and prints where.

Exit codes: 0 success, 1 failure, 2 usage error (unknown command, missing arguments).

Examples:
    scripts/app-drive.sh doctor
    scripts/app-drive.sh start --json
    scripts/app-drive.sh rize -- --json entries list --from today
    scripts/app-drive.sh shot --out /tmp/proof/settings.png
    scripts/app-drive.sh stop --all --dry-run
USAGE
}

die() {
  echo "app-drive: $*" >&2
  exit 1
}

# A usage error (unknown command, missing arguments) exits 2 so callers can tell
# it from a runtime failure.
die_usage() {
  echo "app-drive: $*" >&2
  exit 2
}

# Quote a value as a JSON string: escapes backslash, double quote, newline, tab
# and carriage return, and drops any other control character.
json_string() {
  local s="$1"
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//$'\n'/\\n}
  s=${s//$'\t'/\\t}
  s=${s//$'\r'/\\r}
  s="$(printf '%s' "$s" | LC_ALL=C tr -d '\000-\037')"
  printf '"%s"' "$s"
}

# ---------------------------------------------------------------- session state

session_dir() { echo "$SESSION_ROOT/$1"; }

# Every session directory, live or stale (a stale one is left by a dead process).
all_sessions() {
  [[ -d "$SESSION_ROOT" ]] || return 0
  local dir
  for dir in "$SESSION_ROOT"/*; do
    [[ -d "$dir" ]] || continue
    basename "$dir"
  done
}

live_sessions() {
  local name
  while read -r name; do
    [[ -n "$name" ]] || continue
    if session_pid "$name" >/dev/null 2>&1; then
      echo "$name"
    fi
  done < <(all_sessions)
}

# With a second argument, a session directory without a meta file still loads
# (with empty fields), so stop --all can clear it.
load_session() {
  local id="$1" lenient="${2:-}" dir
  dir="$(session_dir "$id")"
  PORT="" PGID="" IDENTIFIER="" APP_DATA="" CONFIG_HOME=""
  if [[ -f "$dir/meta" ]]; then
    # shellcheck disable=SC1090
    source "$dir/meta"
  elif [[ -z "$lenient" ]]; then
    die "no such session: $id"
  fi
  SESSION_ID="$id"
  SESSION_DIR="$dir"
}

# The app binary is the only process in the session's process group matching the
# dev binary path, which makes the session -> pid mapping exact even when other
# agents are running their own instances concurrently.
session_pid() {
  local dir pgid pid
  dir="$(session_dir "$1")"
  [[ -f "$dir/meta" ]] || return 1
  pgid="$(sed -n 's/^PGID="\{0,1\}\([0-9]*\)"\{0,1\}$/\1/p' "$dir/meta")"
  [[ -n "$pgid" ]] || return 1
  pid="$(pgrep -g "$pgid" -f 'target/debug/openrize$' 2>/dev/null | head -1 || true)"
  [[ -n "$pid" ]] || return 1
  echo "$pid"
}

resolve_session() {
  if [[ -n "${OPT_SESSION:-}" ]]; then
    echo "$OPT_SESSION"
    return
  fi
  if [[ -n "${OPENRIZE_SESSION:-}" ]]; then
    echo "$OPENRIZE_SESSION"
    return
  fi
  local found
  found="$(live_sessions)"
  case "$(echo "$found" | grep -c . || true)" in
    0) die "no live session - run 'scripts/app-drive.sh start' first" ;;
    1) echo "$found" ;;
    *) die "several live sessions ($(echo "$found" | tr '\n' ' ')) - pass -s ID" ;;
  esac
}

# Resolve the session and its pid, and export what ax-drive.js reads.
attach() {
  local id
  id="$(resolve_session)" || exit 1
  load_session "$id"
  APP_PID="$(session_pid "$id")" || die "session $id is no longer running"
}

ax() {
  local errfile rc=0
  errfile="$(mktemp "${TMPDIR:-/tmp}/app-drive-ax.XXXXXX")"
  AX_PID="$APP_PID" AX_CMD="$1" AX_TARGET="${2:-}" AX_ROLE="${OPT_ROLE:-}" AX_INDEX="${OPT_INDEX:-}" \
    AX_FULL="${OPT_FULL:-}" osascript -l JavaScript "$AX_DRIVE_JS" 2>"$errfile" || rc=$?
  if [[ $rc -ne 0 ]] && grep -qiE 'assistive access|not allowed|-1719|-25211|not authorized' "$errfile"; then
    rm -f "$errfile"
    die "Accessibility is not granted to the terminal running this. Grant it in System Settings > Privacy & Security > Accessibility, then run 'scripts/app-drive.sh doctor'."
  fi
  cat "$errfile" >&2
  rm -f "$errfile"
  return "$rc"
}

# ---------------------------------------------------------------------- doctor

CHECK_IDS=()
CHECK_STATUS=()
CHECK_DETAIL=()
CHECK_FIX=()

# add_check <id> <ok|warn|fail> <detail> [fix]
add_check() {
  CHECK_IDS+=("$1")
  CHECK_STATUS+=("$2")
  CHECK_DETAIL+=("$3")
  CHECK_FIX+=("${4:-}")
}

# Run a JXA probe that prints true or false. Prints the probe's output (or its
# error text) and returns 1 when osascript itself failed.
osa_probe() {
  local out
  if out="$(osascript -l JavaScript -e "$1" 2>&1)"; then
    printf '%s' "$out"
  else
    printf '%s' "$out"
    return 1
  fi
}

# check_permission <id> <fail|warn> <probe source> <note on the bad case> <fix text>
check_permission() {
  local id="$1" bad="$2" source="$3" note="$4" fix="$5" out
  local rerun="rerun scripts/app-drive.sh doctor; if it keeps failing, the probe itself is broken - report the detail above"
  if ! command -v osascript >/dev/null 2>&1; then
    add_check "$id" "$bad" "osascript not found (macOS only)" "$fix"
    return
  fi
  if out="$(osa_probe "$source")"; then
    case "$out" in
      true) add_check "$id" ok "granted" ;;
      false) add_check "$id" "$bad" "not granted$note" "$fix" ;;
      *) add_check "$id" fail "unexpected probe output: $out" "$rerun" ;;
    esac
  else
    add_check "$id" fail "probe failed: $out" "$rerun"
  fi
}

check_command() {
  local id="$1" bad="$2" fix="$3" path
  if path="$(command -v "$id" 2>/dev/null)"; then
    add_check "$id" ok "$path"
  else
    add_check "$id" "$bad" "$id not found on PATH" "$fix"
  fi
}

cmd_doctor() {
  local terminal="the terminal app running this"
  [[ -z "${TERM_PROGRAM:-}" ]] || terminal="the terminal app running this ($TERM_PROGRAM)"

  if [[ "$(uname)" == "Darwin" ]]; then
    add_check macos ok "Darwin"
  else
    add_check macos fail "app-drive.sh only works on macOS (no supported Tauri WebDriver backend here)" \
      "run this on a Mac (on Windows use node scripts/app-drive-windows.mjs)"
  fi

  if [[ -e "$REPO_ROOT/node_modules/.bin/tauri" ]]; then
    add_check node-modules ok "$REPO_ROOT/node_modules/.bin/tauri"
  else
    add_check node-modules fail "node_modules/.bin/tauri is missing" "pnpm install"
  fi
  check_command pnpm fail "install pnpm (https://pnpm.io/installation)"
  check_command cargo fail "install Rust with rustup (https://rustup.rs)"

  check_permission accessibility fail \
    'ObjC.import("ApplicationServices"); $.AXIsProcessTrusted()' "" \
    "grant Accessibility to $terminal in System Settings > Privacy & Security > Accessibility, then rerun scripts/app-drive.sh doctor"
  check_permission screen-recording warn \
    'ObjC.import("CoreGraphics"); ObjC.bindFunction("CGPreflightScreenCaptureAccess", ["bool", []]); $.CGPreflightScreenCaptureAccess()' " (only shot needs it)" \
    "grant Screen Recording to $terminal in System Settings > Privacy & Security > Screen & System Audio Recording, then rerun scripts/app-drive.sh doctor"

  check_command sqlite3 warn "only needed to read the database directly; install sqlite3 if you want that"

  if [[ -e "$REPO_ROOT/src-tauri/target/debug/openrize" ]]; then
    add_check warm-build ok "$REPO_ROOT/src-tauri/target/debug/openrize"
  else
    add_check warm-build warn "src-tauri/target/debug/openrize is missing: the first start compiles the whole Rust tree (can take minutes; start waits OPENRIZE_DRIVE_TIMEOUT, default ${STARTUP_TIMEOUT}s)" \
      "expect a long first start, or raise OPENRIZE_DRIVE_TIMEOUT"
  fi

  local id live=() stale=()
  while read -r id; do
    [[ -n "$id" ]] || continue
    if session_pid "$id" >/dev/null 2>&1; then live+=("$id"); else stale+=("$id"); fi
  done < <(all_sessions)
  if [[ ${#stale[@]} -gt 0 ]]; then
    add_check sessions warn "${#live[@]} live, ${#stale[@]} stale (directory left by a dead process): ${stale[*]}" \
      "scripts/app-drive.sh stop --all"
  elif [[ ${#live[@]} -gt 0 ]]; then
    add_check sessions ok "${#live[@]} live: ${live[*]}"
  else
    add_check sessions ok "no sessions"
  fi

  local i failures=0
  for ((i = 0; i < ${#CHECK_IDS[@]}; i++)); do
    [[ "${CHECK_STATUS[$i]}" != "fail" ]] || failures=$((failures + 1))
  done

  if [[ -n "${OPT_JSON:-}" ]]; then
    local ok=true sep=""
    [[ $failures -eq 0 ]] || ok=false
    printf '{"ok":%s,"checks":[' "$ok"
    for ((i = 0; i < ${#CHECK_IDS[@]}; i++)); do
      printf '%s{"id":%s,"status":%s,"detail":%s' "$sep" "$(json_string "${CHECK_IDS[$i]}")" \
        "$(json_string "${CHECK_STATUS[$i]}")" "$(json_string "${CHECK_DETAIL[$i]}")"
      [[ "${CHECK_STATUS[$i]}" == "ok" ]] || printf ',"fix":%s' "$(json_string "${CHECK_FIX[$i]}")"
      printf '}'
      sep=","
    done
    printf ']}\n'
  else
    for ((i = 0; i < ${#CHECK_IDS[@]}; i++)); do
      printf '%-5s%s  %s\n' "${CHECK_STATUS[$i]}" "${CHECK_IDS[$i]}" "${CHECK_DETAIL[$i]}"
      [[ "${CHECK_STATUS[$i]}" == "ok" ]] || printf '     fix: %s\n' "${CHECK_FIX[$i]}"
    done
    if [[ $failures -eq 0 ]]; then
      echo "doctor: ready"
    else
      echo "doctor: $failures problem(s) - fix the lines marked fail, then rerun doctor"
    fi
  fi
  [[ $failures -eq 0 ]]
}

# --------------------------------------------------------------------- commands

cmd_start() {
  mkdir -p "$SESSION_ROOT"

  local port=""
  local candidate
  for _ in $(seq 1 40); do
    candidate=$(((RANDOM % 20000) + 20000))
    if ! lsof -iTCP:"$candidate" -sTCP:LISTEN >/dev/null 2>&1 && [[ ! -d "$SESSION_ROOT/d$candidate" ]]; then
      port="$candidate"
      break
    fi
  done
  [[ -n "$port" ]] || die "could not find a free port"

  local id="d$port"
  local dir="$SESSION_ROOT/$id"
  local identifier="com.offlinestudios.openrize.drive$port"
  rm -rf "$dir"
  mkdir -p "$dir/config" "$dir/shots"

  # Two separate isolation mechanisms, both required:
  #   identifier      -> Tauri's app_data_dir, which holds activity.db and timers
  #   XDG_CONFIG_HOME -> settings.rs::config_dir(), which is NOT identifier-scoped
  #                      and would otherwise be the real ~/.config/openrize
  local overrides
  overrides=$(printf '{"identifier":"%s","build":{"beforeDevCommand":"pnpm exec vite --port %s --strictPort","devUrl":"http://localhost:%s"}}' \
    "$identifier" "$port" "$port")

  echo "==> starting isolated instance $id on port $port" >&2
  XDG_CONFIG_HOME="$dir/config" nohup pnpm tauri dev --no-watch -c "$overrides" >"$dir/dev.log" 2>&1 &
  local pgid=$!

  cat >"$dir/meta" <<META
PORT="$port"
PGID="$pgid"
IDENTIFIER="$identifier"
APP_DATA="$HOME/Library/Application Support/$identifier"
CONFIG_HOME="$dir/config"
META

  local pid=""
  for _ in $(seq 1 "$STARTUP_TIMEOUT"); do
    pid="$(pgrep -g "$pgid" -f 'target/debug/openrize$' 2>/dev/null | head -1 || true)"
    [[ -n "$pid" ]] && break
    if ! kill -0 -- -"$pgid" 2>/dev/null; then
      echo "--- last 20 log lines ---" >&2
      tail -20 "$dir/dev.log" >&2
      die "the dev process exited before the app started (log: $dir/dev.log)"
    fi
    sleep 1
  done
  [[ -n "$pid" ]] || die "app binary never started within ${STARTUP_TIMEOUT}s (log: $dir/dev.log)"

  APP_PID="$pid"
  # The process existing is not the same as the webview being laid out; wait for
  # the window, then for the web content itself to publish its tree. A missing
  # Accessibility grant would otherwise look like a window that never appears.
  local ready="" tree_out
  for _ in $(seq 1 40); do
    tree_out="$(ax tree 2>&1 || true)"
    if grep -q '\[Button\]' <<<"$tree_out"; then
      ready=1
      break
    fi
    if grep -q 'Accessibility is not granted' <<<"$tree_out"; then
      die "the app started (session $id is still running; 'scripts/app-drive.sh stop -s $id' removes it) but Accessibility is not granted to the terminal running this. Grant it in System Settings > Privacy & Security > Accessibility, then run 'scripts/app-drive.sh doctor'."
    fi
    sleep 1
  done
  [[ -n "$ready" ]] || die "window never became drivable (log: $dir/dev.log)"

  if [[ -n "${OPT_JSON:-}" ]]; then
    printf '{"session":%s,"port":%s,"pid":%s,"identifier":%s,"config":%s,"appData":%s,"log":%s}\n' \
      "$(json_string "$id")" "$port" "$pid" "$(json_string "$identifier")" "$(json_string "$dir/config")" \
      "$(json_string "$HOME/Library/Application Support/$identifier")" "$(json_string "$dir/dev.log")"
  else
    echo "session=$id port=$port pid=$pid identifier=$identifier"
    echo "config=$dir/config app-data=$HOME/Library/Application Support/$identifier"
  fi
}

cmd_list() {
  local id found="" sep=""
  [[ -z "${OPT_JSON:-}" ]] || printf '{"sessions":['
  while read -r id; do
    [[ -n "$id" ]] || continue
    found=1
    load_session "$id"
    if [[ -n "${OPT_JSON:-}" ]]; then
      printf '%s{"session":%s,"port":%s,"pid":%s,"identifier":%s,"appData":%s}' "$sep" \
        "$(json_string "$id")" "$PORT" "$(session_pid "$id")" "$(json_string "$IDENTIFIER")" "$(json_string "$APP_DATA")"
      sep=","
    else
      echo "$id  port=$PORT  pid=$(session_pid "$id")  identifier=$IDENTIFIER"
    fi
  done < <(live_sessions)
  if [[ -n "${OPT_JSON:-}" ]]; then
    printf ']}\n'
  elif [[ -z "$found" ]]; then
    echo "no live sessions for $REPO_ROOT"
  fi
}

cmd_tree() {
  attach
  ax tree
}

cmd_find() {
  [[ $# -ge 1 ]] || die_usage "find needs a name, e.g. scripts/app-drive.sh find \"Settings\""
  attach
  ax find "$1"
}

cmd_click() {
  [[ $# -ge 1 ]] || die_usage "click needs a name, e.g. scripts/app-drive.sh click \"Settings\""
  attach
  ax click "$1"
}

cmd_fill() {
  [[ $# -ge 2 ]] || die_usage "fill needs a field name and text, e.g. scripts/app-drive.sh fill \"Search\" \"hello\""
  attach
  # AXPress focuses the field exactly as a user's click would. Writing AXValue
  # directly is not an option: WebKit accepts the write, silently drops it, and
  # React would never see an onChange even if it landed.
  OPT_ROLE="${OPT_ROLE:-AXTextField}" ax click "$1" >/dev/null
  # Keystrokes are global, so raising the window and typing have to happen inside
  # one System Events block; anything in between can hand focus to another app.
  osascript -e "tell application \"System Events\"
set frontmost of (first process whose unix id is $APP_PID) to true
delay 0.5
keystroke $(osa_string "$2")
end tell" >/dev/null
  echo "typed into $1"
}

cmd_key() {
  [[ $# -ge 1 ]] || die_usage "key needs a keyspec, e.g. scripts/app-drive.sh key cmd+comma"
  attach

  local spec="$1"
  local modifiers=()
  while [[ "$spec" == *+* ]]; do
    case "${spec%%+*}" in
      cmd | command) modifiers+=("command down") ;;
      shift) modifiers+=("shift down") ;;
      opt | option | alt) modifiers+=("option down") ;;
      ctrl | control) modifiers+=("control down") ;;
      *) die "unknown modifier ${spec%%+*}" ;;
    esac
    spec="${spec#*+}"
  done

  local action
  case "$spec" in
    return | enter) action="key code 36" ;;
    tab) action="key code 48" ;;
    space) action="key code 49" ;;
    escape | esc) action="key code 53" ;;
    backspace) action="key code 51" ;;
    delete | forwarddelete) action="key code 117" ;;
    left) action="key code 123" ;;
    right) action="key code 124" ;;
    down) action="key code 125" ;;
    up) action="key code 126" ;;
    comma) action="keystroke \",\"" ;;
    *) action="keystroke $(osa_string "$spec")" ;;
  esac

  local using=""
  if [[ ${#modifiers[@]} -gt 0 ]]; then
    local joined
    joined="$(printf '%s, ' "${modifiers[@]}")"
    using=" using {${joined%, }}"
  fi

  osascript -e "tell application \"System Events\"
set frontmost of (first process whose unix id is $APP_PID) to true
delay 0.5
$action$using
end tell" >/dev/null
  echo "sent $1"
}

cmd_wait() {
  [[ $# -ge 1 ]] || die_usage "wait needs text to look for, e.g. scripts/app-drive.sh wait \"Settings\" 15"
  attach
  local needle="$1" limit="${2:-15}"
  for _ in $(seq 1 "$limit"); do
    if ax tree | grep -qiF -- "$needle"; then
      echo "found: $needle"
      return 0
    fi
    sleep 1
  done
  die "timed out after ${limit}s waiting for: $needle"
}

cmd_shot() {
  local out="${OPT_OUT:-}"
  if [[ -n "$out" && "$out" != *.png ]]; then
    die_usage "--out must end in .png, e.g. scripts/app-drive.sh shot --out /tmp/proof/settings.png"
  fi
  attach
  # Capture by CGWindowID rather than by screen rectangle: this window is grabbed
  # correctly even when another agent's instance sits on top of it, and nothing
  # has to be raised or moved, so a concurrent session's focus is left alone.
  local window_id
  window_id="$(ax windowid)" || die "could not resolve the window id for session $SESSION_ID"
  if [[ -z "$out" ]]; then
    out="$SESSION_DIR/shots/$(date +%s)-$RANDOM.png"
  else
    mkdir -p "$(dirname "$out")"
  fi
  if ! screencapture -x -o -l"$window_id" "$out" || [[ ! -s "$out" ]]; then
    rm -f "$out"
    die "screencapture produced no image. Grant Screen Recording to the terminal running this in System Settings > Privacy & Security > Screen & System Audio Recording, then run 'scripts/app-drive.sh doctor'."
  fi
  echo "$out"
}

cmd_logs() {
  attach
  tail -"${1:-40}" "$SESSION_DIR/dev.log"
}

cmd_rize() {
  if [[ -z "${HAVE_DASHDASH:-}" || ${#RIZE_ARGS[@]} -eq 0 || ${#ARGS[@]} -gt 0 ]]; then
    die_usage "rize needs '--' followed by the rize arguments, e.g. scripts/app-drive.sh rize -- --json entries list --from today"
  fi
  attach
  local rc=0
  RIZE_APP="$SESSION_DIR/no-app-launch" cargo run --quiet --manifest-path "$REPO_ROOT/src-tauri/Cargo.toml" \
    -p openrize-cli --bin rize -- --data-dir "$APP_DATA" "${RIZE_ARGS[@]}" || rc=$?
  exit "$rc"
}

# Check what stop is about to delete for the loaded session. Prints the reason
# and returns 1 when the meta file looks corrupted.
session_guard() {
  local support="$HOME/Library/Application Support" name
  if [[ -n "$APP_DATA" ]]; then
    name="$(basename "$APP_DATA")"
    if ! [[ "$name" =~ ^com\.offlinestudios\.openrize\.drive[0-9]+$ ]] || [[ "$(dirname "$APP_DATA")" != "$support" ]]; then
      echo "refusing to delete '$APP_DATA': the meta file of session $SESSION_ID looks corrupted (APP_DATA is not a drive directory under '$support'); nothing was deleted for that session"
      return 1
    fi
  fi
  name="$(basename "$SESSION_DIR")"
  if [[ "$(dirname "$SESSION_DIR")" != "$SESSION_ROOT" || "$name" == "." || "$name" == ".." ]]; then
    echo "refusing to delete '$SESSION_DIR': the meta file of session $SESSION_ID looks corrupted (the session directory is not directly under '$SESSION_ROOT'); nothing was deleted for that session"
    return 1
  fi
}

cmd_stop() {
  local json="${OPT_JSON:-}" dry="${OPT_DRYRUN:-}"
  local ids=() id lenient=""
  if [[ "${OPT_ALL:-}" == "1" ]]; then
    lenient=1
    while read -r id; do [[ -n "$id" ]] && ids+=("$id"); done < <(all_sessions)
    if [[ ${#ids[@]} -eq 0 ]]; then
      if [[ -n "$json" ]]; then
        printf '{"dryRun":%s,"sessions":[]}\n' "$([[ -n "$dry" ]] && echo true || echo false)"
      else
        echo "no live sessions for $REPO_ROOT"
      fi
      return 0
    fi
  else
    local only
    only="$(resolve_session)" || exit 1
    ids=("$only")
  fi

  # Refuse before touching anything, so a corrupted session never leaves a
  # half-stopped batch behind. A dry run reports the refusal instead.
  local reason
  if [[ -z "$dry" ]]; then
    for id in "${ids[@]}"; do
      load_session "$id" "$lenient"
      reason="$(session_guard)" || die "$reason"
    done
  fi

  local pgid state refused shots evidence evidence_dir deleted sep="" path
  [[ -z "$json" ]] || printf '{"dryRun":%s,"sessions":[' "$([[ -n "$dry" ]] && echo true || echo false)"
  for id in "${ids[@]}"; do
    load_session "$id" "$lenient"
    pgid="$PGID"
    [[ "$pgid" =~ ^[0-9]+$ ]] || pgid=""
    state=stale
    if session_pid "$id" >/dev/null 2>&1; then state=live; fi
    refused=""
    reason="$(session_guard)" || refused="$reason"

    deleted=()
    evidence=""
    if [[ -z "$refused" ]]; then
      evidence_dir="${TMPDIR:-/tmp}/openrize-app-drive-evidence/$id"
      shots=("$SESSION_DIR"/shots/*.png)
      [[ ! -e "${shots[0]}" ]] || evidence="$evidence_dir"
      if [[ -n "$APP_DATA" && -d "$APP_DATA" ]]; then deleted+=("$APP_DATA"); fi
      if [[ -d "$SESSION_DIR" ]]; then deleted+=("$SESSION_DIR"); fi
    fi

    if [[ -z "$dry" ]]; then
      if [[ -n "$pgid" && "$pgid" -gt 1 ]] && kill -0 -- -"$pgid" 2>/dev/null; then
        kill -TERM -- -"$pgid" 2>/dev/null || true
        sleep 1
        kill -KILL -- -"$pgid" 2>/dev/null || true
      fi
      if [[ -n "$evidence" ]]; then
        mkdir -p "$evidence"
        mv "${shots[@]}" "$evidence/"
        [[ -n "$json" ]] || echo "kept evidence in $evidence"
      fi
      for path in ${deleted[@]+"${deleted[@]}"}; do rm -rf "$path"; done
      [[ -n "$json" ]] || echo "stopped $id"
    elif [[ -z "$json" ]]; then
      if [[ -n "$refused" ]]; then
        echo "would refuse to stop $id ($state): ${refused#refusing to delete }"
      else
        echo "would stop $id ($state) pgid=${pgid:-none}"
        [[ -z "$evidence" ]] || echo "  would keep evidence in $evidence"
        for path in ${deleted[@]+"${deleted[@]}"}; do echo "  would delete $path"; done
      fi
    fi

    if [[ -n "$json" ]]; then
      printf '%s{"session":%s,"state":%s,"pgid":%s,"deleted":[' "$sep" "$(json_string "$id")" "$(json_string "$state")" "${pgid:-null}"
      local dsep=""
      for path in ${deleted[@]+"${deleted[@]}"}; do
        printf '%s%s' "$dsep" "$(json_string "$path")"
        dsep=","
      done
      printf ']'
      [[ -z "$evidence" ]] || printf ',"evidence":%s' "$(json_string "$evidence")"
      [[ -z "$refused" ]] || printf ',"refused":%s' "$(json_string "$refused")"
      printf '}'
      sep=","
    fi
  done
  if [[ -n "$json" ]]; then
    printf ']}\n'
  elif [[ -n "$dry" ]]; then
    echo "dry run: nothing was stopped or deleted"
  fi
}

# Quote a value for embedding in AppleScript source.
osa_string() {
  printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
}

# ------------------------------------------------------------------ entry point

[[ $# -ge 1 ]] || {
  usage
  exit 2
}
COMMAND="$1"
shift

ARGS=()
RIZE_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -s | --session | -r | --role | -i | --index | --out)
      [[ $# -ge 2 ]] || die_usage "$1 needs a value, e.g. scripts/app-drive.sh tree -s d12345"
      case "$1" in
        -s | --session) OPT_SESSION="$2" ;;
        -r | --role) OPT_ROLE="$2" ;;
        -i | --index) OPT_INDEX="$2" ;;
        --out) OPT_OUT="$2" ;;
      esac
      shift 2
      ;;
    --full)
      OPT_FULL=1
      shift
      ;;
    --all)
      OPT_ALL=1
      shift
      ;;
    --json)
      OPT_JSON=1
      shift
      ;;
    --dry-run)
      OPT_DRYRUN=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    --)
      if [[ "$COMMAND" == "rize" ]]; then
        # Everything after the first -- is rize's, verbatim.
        shift
        HAVE_DASHDASH=1
        RIZE_ARGS=("$@")
        break
      fi
      ARGS+=("$1")
      shift
      ;;
    *)
      ARGS+=("$1")
      shift
      ;;
  esac
done

case "$COMMAND" in
  doctor | -h | --help | help) ;;
  start | list | tree | find | click | fill | key | wait | shot | logs | rize | stop)
    if [[ "$(uname)" != "Darwin" ]]; then
      die "app-drive.sh only works on macOS (no supported Tauri WebDriver backend here)"
    fi
    ;;
esac

case "$COMMAND" in
  doctor) cmd_doctor ;;
  start) cmd_start ;;
  list) cmd_list ;;
  tree) cmd_tree ;;
  find) cmd_find ${ARGS[@]+"${ARGS[@]}"} ;;
  click) cmd_click ${ARGS[@]+"${ARGS[@]}"} ;;
  fill) cmd_fill ${ARGS[@]+"${ARGS[@]}"} ;;
  key) cmd_key ${ARGS[@]+"${ARGS[@]}"} ;;
  wait) cmd_wait ${ARGS[@]+"${ARGS[@]}"} ;;
  shot) cmd_shot ;;
  logs) cmd_logs ${ARGS[@]+"${ARGS[@]}"} ;;
  rize) cmd_rize ;;
  stop) cmd_stop ;;
  -h | --help | help)
    usage
    exit 0
    ;;
  *)
    echo "app-drive: unknown command '$COMMAND'" >&2
    usage >&2
    exit 2
    ;;
esac
