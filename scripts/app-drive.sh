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

if [[ "$(uname)" != "Darwin" ]]; then
  echo "app-drive.sh only works on macOS (no supported Tauri WebDriver backend here)" >&2
  exit 1
fi

WORKSPACE_KEY="$(printf '%s' "$REPO_ROOT" | shasum | cut -c1-10)"
SESSION_ROOT="${TMPDIR:-/tmp}/openrize-app-drive/$WORKSPACE_KEY"
AX_DRIVE_JS="$REPO_ROOT/scripts/ax-drive.js"

# How long to wait for the app binary to appear. A warm target/ dir gets there in
# seconds; a cold `cargo build` of the whole dependency tree can take minutes.
STARTUP_TIMEOUT="${OPENRIZE_DRIVE_TIMEOUT:-300}"

usage() {
  cat <<'USAGE'
usage: scripts/app-drive.sh <command> [args]

  start                       launch an isolated instance, print its session id
  list                        list live sessions started from this checkout
  tree   [-s ID] [--full]     print the window's accessibility tree
  find   [-s ID] <name>       resolve a control by its accessible name
  click  [-s ID] <name>       click a control (real DOM click via AXPress)
  fill   [-s ID] <name> <txt> focus a field and type text into it
  key    [-s ID] <keyspec>    send a keystroke, e.g. return, tab, cmd+comma
  wait   [-s ID] <text> [sec] block until text appears in the tree (default 15s)
  shot   [-s ID]              screenshot the window, print the PNG path
  logs   [-s ID] [n]          tail the instance's dev-server/cargo log
  stop   [-s ID | --all]      kill the instance and delete its data

Options common to the driving commands:
  -s, --session ID   target a specific session (default: the only live one)
  -r, --role ROLE    restrict matching to an AX role, e.g. AXRadioButton
  -i, --index N      pick the Nth match when a name is ambiguous
USAGE
}

die() {
  echo "app-drive: $*" >&2
  exit 1
}

# ---------------------------------------------------------------- session state

session_dir() { echo "$SESSION_ROOT/$1"; }

live_sessions() {
  [[ -d "$SESSION_ROOT" ]] || return 0
  local dir name
  for dir in "$SESSION_ROOT"/*; do
    [[ -d "$dir" ]] || continue
    name="$(basename "$dir")"
    if session_pid "$name" >/dev/null 2>&1; then
      echo "$name"
    fi
  done
}

load_session() {
  local id="$1" dir
  dir="$(session_dir "$id")"
  [[ -f "$dir/meta" ]] || die "no such session: $id"
  # shellcheck disable=SC1090
  source "$dir/meta"
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
  AX_PID="$APP_PID" AX_CMD="$1" AX_TARGET="${2:-}" AX_ROLE="${OPT_ROLE:-}" AX_INDEX="${OPT_INDEX:-}" \
    AX_FULL="${OPT_FULL:-}" osascript -l JavaScript "$AX_DRIVE_JS"
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
  local identifier="com.elgohr.openrize.drive$port"
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
  # the window, then for the web content itself to publish its tree.
  local ready=""
  for _ in $(seq 1 40); do
    if ax tree 2>/dev/null | grep -q '\[Button\]'; then
      ready=1
      break
    fi
    sleep 1
  done
  [[ -n "$ready" ]] || die "window never became drivable (log: $dir/dev.log)"

  echo "session=$id port=$port pid=$pid identifier=$identifier"
  echo "config=$dir/config app-data=$HOME/Library/Application Support/$identifier"
}

cmd_list() {
  local id found=""
  while read -r id; do
    [[ -n "$id" ]] || continue
    found=1
    load_session "$id"
    echo "$id  port=$PORT  pid=$(session_pid "$id")  identifier=$IDENTIFIER"
  done < <(live_sessions)
  [[ -n "$found" ]] || echo "no live sessions for $REPO_ROOT"
}

cmd_tree() {
  attach
  ax tree
}

cmd_find() {
  attach
  [[ $# -ge 1 ]] || die "find needs a name"
  ax find "$1"
}

cmd_click() {
  attach
  [[ $# -ge 1 ]] || die "click needs a name"
  ax click "$1"
}

cmd_fill() {
  attach
  [[ $# -ge 2 ]] || die "fill needs a field name and text"
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
  attach
  [[ $# -ge 1 ]] || die "key needs a keyspec"

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
  attach
  [[ $# -ge 1 ]] || die "wait needs text to look for"
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
  attach
  # Capture by CGWindowID rather than by screen rectangle: this window is grabbed
  # correctly even when another agent's instance sits on top of it, and nothing
  # has to be raised or moved, so a concurrent session's focus is left alone.
  local window_id out
  window_id="$(ax windowid)" || die "could not resolve the window id for session $SESSION_ID"
  out="$SESSION_DIR/shots/$(date +%s)-$RANDOM.png"
  screencapture -x -o -l"$window_id" "$out"
  echo "$out"
}

cmd_logs() {
  attach
  tail -"${1:-40}" "$SESSION_DIR/dev.log"
}

cmd_stop() {
  local ids=()
  if [[ "${OPT_ALL:-}" == "1" ]]; then
    while read -r id; do [[ -n "$id" ]] && ids+=("$id"); done < <(live_sessions)
    [[ ${#ids[@]} -gt 0 ]] || {
      echo "no live sessions for $REPO_ROOT"
      return 0
    }
  else
    local only
    only="$(resolve_session)" || exit 1
    ids=("$only")
  fi

  local id pgid
  for id in "${ids[@]}"; do
    load_session "$id"
    pgid="$PGID"
    if kill -0 -- -"$pgid" 2>/dev/null; then
      kill -TERM -- -"$pgid" 2>/dev/null || true
      sleep 1
      kill -KILL -- -"$pgid" 2>/dev/null || true
    fi
    [[ -n "$APP_DATA" ]] && rm -rf "$APP_DATA"
    [[ -n "$SESSION_DIR" ]] && rm -rf "$SESSION_DIR"
    echo "stopped $id"
  done
}

# Quote a value for embedding in AppleScript source.
osa_string() {
  printf '"%s"' "$(printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g')"
}

# ------------------------------------------------------------------ entry point

[[ $# -ge 1 ]] || {
  usage
  exit 1
}
COMMAND="$1"
shift

ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    -s | --session)
      OPT_SESSION="$2"
      shift 2
      ;;
    -r | --role)
      OPT_ROLE="$2"
      shift 2
      ;;
    -i | --index)
      OPT_INDEX="$2"
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
    -h | --help)
      usage
      exit 0
      ;;
    *)
      ARGS+=("$1")
      shift
      ;;
  esac
done

case "$COMMAND" in
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
  stop) cmd_stop ;;
  -h | --help | help)
    usage
    exit 0
    ;;
  *)
    usage
    exit 1
    ;;
esac
