#!/usr/bin/env bash
# Shoots the OpenRize window in dark and light for offlinestudios.net, from the seeded day in
# seed.json. macOS only (it drives the window with System Events and screencapture) and it
# needs ImageMagick (`magick`) and `sqlite3`.
#
#   docs/website-screenshots/capture.sh [checkout] [out-dir]
#
# checkout  the OpenRize checkout to run, at the commit the seed was designed for (see README.md).
#           Defaults to the repo this script lives in.
# out-dir   where openrize-dark.png and openrize-light.png go. Defaults to ./website-screenshots-out.
#
# Nothing here touches your real OpenRize: the app runs under its own identifier (its own
# database) and its own XDG_CONFIG_HOME (its own settings.json, which is where theme lives).
# The checkout's index.html is patched to freeze the clock and restored on exit.
set -euo pipefail
set -m

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECKOUT="$(cd "${1:-$HERE/../..}" && pwd)"
OUT_DIR="$(mkdir -p "${2:-./website-screenshots-out}" && cd "${2:-./website-screenshots-out}" && pwd)"

IDENTIFIER="com.offlinestudios.openrize.websiteshot"
DATA_DIR="$HOME/Library/Application Support/$IDENTIFIER"
CONFIG_HOME="$(mktemp -d)"
PORT=29183
# The window is 1492 x 937 pt. At 2x, with the system shadow, it is composed onto a 1604 x 1050 canvas
# with its top-left corner at (56, 38): the framing the site's screenshot has always had.
WIN_W=1492
WIN_H=937
CANVAS_W=1604
CANVAS_H=1050
MARGIN_X=56
MARGIN_Y=38

command -v magick >/dev/null || { echo "ImageMagick (magick) is required" >&2; exit 1; }
cd "$CHECKOUT"

# Freeze the app's clock at 4:41 PM on the seeded day (see freeze-time.js).
cp index.html "$CONFIG_HOME/index.html.orig"
restore() {
  cp "$CONFIG_HOME/index.html.orig" "$CHECKOUT/index.html"
  stop_app
  rm -rf "$CONFIG_HOME"
}
trap restore EXIT
python3 - "$HERE/freeze-time.js" <<'PY'
import sys
js = open(sys.argv[1]).read()
html = open("index.html").read()
open("index.html", "w").write(html.replace("<head>", "<head>\n    <script>" + js + "</script>", 1))
PY

OVERRIDE=$(printf '{"identifier":"%s","build":{"beforeDevCommand":"pnpm exec vite --port %s --strictPort","devUrl":"http://localhost:%s"},"app":{"windows":[{"title":"OpenRize","width":%s,"height":%s,"minWidth":720,"minHeight":560,"titleBarStyle":"Overlay","hiddenTitle":true,"trafficLightPosition":{"x":16,"y":19},"x":10,"y":36}]}}' "$IDENTIFIER" "$PORT" "$PORT" "$WIN_W" "$WIN_H")

DEV_PGID=""
stop_app() {
  if [[ -n "$DEV_PGID" ]] && kill -0 -- -"$DEV_PGID" 2>/dev/null; then
    kill -TERM -- -"$DEV_PGID" 2>/dev/null || true
    sleep 1
    kill -KILL -- -"$DEV_PGID" 2>/dev/null || true
    wait "$DEV_PGID" 2>/dev/null || true
  fi
  DEV_PGID=""
  pkill -f "vite --port $PORT" 2>/dev/null || true
}

# launch <theme>: starts the app with that theme, waits until its window exists, sets BIN_PID.
launch() {
  mkdir -p "$CONFIG_HOME/openrize"
  printf '{"theme":"%s"}\n' "$1" >"$CONFIG_HOME/openrize/settings.json"
  XDG_CONFIG_HOME="$CONFIG_HOME" pnpm tauri dev --no-watch -c "$OVERRIDE" >"$CONFIG_HOME/dev.log" 2>&1 &
  DEV_PGID=$!
  BIN_PID=""
  for _ in $(seq 1 240); do
    BIN_PID="$(pgrep -n -f 'target/debug/openrize$' || true)"
    [[ -n "$BIN_PID" ]] && break
    sleep 1
  done
  [[ -n "$BIN_PID" ]] || { echo "app never started; see $CONFIG_HOME/dev.log" >&2; exit 1; }
  for _ in $(seq 1 30); do
    [[ "$(osascript -e "tell application \"System Events\" to get exists (first window of (first process whose unix id is $BIN_PID))" 2>/dev/null || true)" == "true" ]] && break
    sleep 1
  done
}

# 1. First launch only creates and migrates the database; then the seed goes in.
if [[ ! -f "$DATA_DIR/activity.db" ]]; then
  launch dark
  for _ in $(seq 1 60); do [[ -f "$DATA_DIR/activity.db" ]] && break; sleep 1; done
  sleep 2
  stop_app
fi
node "$HERE/seed.mjs" "$DATA_DIR/activity.db"

# 2. One shot per theme.
for THEME in dark light; do
  launch "$THEME"
  sleep 8 # let the webview paint and the Calendar load
  osascript -e "tell application \"System Events\"
    set proc to first process whose unix id is $BIN_PID
    set frontmost of proc to true
    set position of window 1 of proc to {10, 36}
  end tell" >/dev/null
  sleep 1
  # Capture by window id, not by screen rectangle: a rectangle grabs whatever is on top.
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
  [[ -n "$WINDOW_ID" ]] || { echo "could not find the app window" >&2; exit 1; }
  RAW="$CONFIG_HOME/raw-$THEME.png"
  screencapture -x -l"$WINDOW_ID" "$RAW"
  # The window is the fully opaque part of the capture; the rest is its shadow.
  BOX="$(magick "$RAW" -alpha extract -threshold 99% -format '%@' info:)"
  BOX_X="${BOX#*+}"; BOX_X="${BOX_X%%+*}"
  BOX_Y="${BOX##*+}"
  magick -size "$((CANVAS_W * 2))x$((CANVAS_H * 2))" xc:none "$RAW" \
    -geometry "$(printf '%+d%+d' "$((MARGIN_X * 2 - BOX_X))" "$((MARGIN_Y * 2 - BOX_Y))")" -composite \
    -resize 50% -strip "$OUT_DIR/openrize-$THEME.png"
  echo "wrote $OUT_DIR/openrize-$THEME.png"
  stop_app
done
