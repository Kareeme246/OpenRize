#!/usr/bin/env bash
# Launches an isolated `tauri dev` instance, screenshots its window once
# ready, tears the whole process tree down, and prints the screenshot path.
#
# Safe to run from multiple agents at the same time: each run gets its own
# free port (so Vite dev servers never collide) and its own Tauri app
# `identifier` (so app-data storage never collides with your real app data
# or with another concurrent run). macOS only — screenshotting relies on
# System Events + screencapture, and there is no supported Tauri WebDriver
# backend for macOS to drive the window instead.
set -euo pipefail
set -m
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [[ "$(uname)" != "Darwin" ]]; then
  echo "dev-screenshot.sh only works on macOS" >&2
  exit 1
fi

OUT_DIR="${TMPDIR:-/tmp}/openrize-dev-screenshot"
mkdir -p "$OUT_DIR"
LOG_FILE="$OUT_DIR/$$.log"
OUT_FILE="$OUT_DIR/$(date +%s)-$$.png"

find_free_port() {
  for _ in $(seq 1 20); do
    local port=$(((RANDOM % 20000) + 20000))
    if ! lsof -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
      echo "$port"
      return 0
    fi
  done
  echo "could not find a free port" >&2
  return 1
}
PORT="$(find_free_port)"

echo "==> starting isolated tauri dev on port $PORT (log: $LOG_FILE)" >&2
CONFIG_OVERRIDE=$(printf '{"identifier":"com.offlinestudios.openrize.devshot%s","build":{"beforeDevCommand":"pnpm exec vite --port %s --strictPort","devUrl":"http://localhost:%s"}}' "$PORT" "$PORT" "$PORT")

pnpm tauri dev --no-watch -c "$CONFIG_OVERRIDE" >"$LOG_FILE" 2>&1 &
DEV_PGID=$!

cleanup() {
  if kill -0 -- -"$DEV_PGID" 2>/dev/null; then
    kill -TERM -- -"$DEV_PGID" 2>/dev/null || true
    sleep 1
    kill -KILL -- -"$DEV_PGID" 2>/dev/null || true
  fi
  wait "$DEV_PGID" 2>/dev/null || true
}
trap cleanup EXIT

# Wait for the compiled binary (not cargo/rustc/vite) to actually be running.
BIN_PID=""
for _ in $(seq 1 90); do
  BIN_PID="$(pgrep -n -f 'target/debug/openrize$' || true)"
  [[ -n "$BIN_PID" ]] && break
  sleep 1
done
if [[ -z "$BIN_PID" ]]; then
  echo "app binary never started within 90s; see $LOG_FILE" >&2
  exit 1
fi

# Give the webview a moment to actually paint after the process exists.
sleep 2

# A raw wry/AppKit window has no AppleScript dictionary of its own, so it's
# addressed through System Events' accessibility proxy — which, unlike a real
# scriptable app, exposes position/size but not a CGWindowID `id` property.
# So: bring it forward and pin it to a known on-screen spot (it may have
# restored a position from a previous run that's now partly off-screen), then
# look its CGWindowID up through CoreGraphics below.
HAS_WINDOW=""
for _ in $(seq 1 20); do
  HAS_WINDOW="$(osascript -e "tell application \"System Events\" to get exists (first window of (first process whose unix id is $BIN_PID))" 2>/dev/null || true)"
  [[ "$HAS_WINDOW" == "true" ]] && break
  sleep 1
done
if [[ "$HAS_WINDOW" != "true" ]]; then
  echo "could not resolve the app window; see $LOG_FILE" >&2
  exit 1
fi

osascript -e "tell application \"System Events\"
set proc to first process whose unix id is $BIN_PID
set frontmost of proc to true
set position of window 1 of proc to {40, 40}
end tell" >/dev/null

# Capture by window id, not by screen rectangle: a rectangle grabs whatever
# is on top there, which is the installed OpenRize whenever it is running.
WINDOW_ID="$(osascript -l JavaScript -e '
ObjC.import("CoreGraphics");
function run(argv) {
  const pid = Number(argv[0]);
  const windows = ObjC.deepUnwrap(ObjC.castRefToObject(
    $.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly, 0)));
  const area = (w) => w.kCGWindowBounds.Width * w.kCGWindowBounds.Height;
  const mine = windows
    .filter((w) => w.kCGWindowOwnerPID === pid && w.kCGWindowLayer === 0)
    .sort((a, b) => area(b) - area(a));
  return mine.length > 0 ? String(mine[0].kCGWindowNumber) : "";
}' "$BIN_PID")"
if [[ -z "$WINDOW_ID" ]]; then
  echo "could not find the app window's id; see $LOG_FILE" >&2
  exit 1
fi

screencapture -x -o -l"$WINDOW_ID" "$OUT_FILE"
echo "$OUT_FILE"
