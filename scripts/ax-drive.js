// JXA accessibility driver for a running OpenRize window. Invoked only by
// scripts/app-drive.sh. See that script and .agents/skills/drive-app/SKILL.md.
//
// Why this exists: OpenRize renders through WKWebView, and System Events'
// `entire contents` stops at the web view boundary (it reports a single empty
// AXGroup). WebKit *does* publish the full web accessibility tree, it is just
// only reachable by walking AXUIElement references directly, which is what
// this file does. That gives coordinate-free, name-based driving of the real
// OS webview: `AXPress` on a web AXButton dispatches a genuine DOM click.
//
// All input arrives via environment variables so arbitrary label text never has
// to survive osascript argument quoting:
//   AX_PID     target process id (required)
//   AX_CMD     tree | find | click | windowid (required)
//   AX_TARGET  accessible name to match (find/click/focus)
//   AX_ROLE    optional AX role filter, e.g. AXRadioButton
//   AX_INDEX   0-based pick when a match is ambiguous
//   AX_FULL    "1" to print every node in `tree`, not just named/actionable ones

ObjC.import("ApplicationServices");
ObjC.import("CoreGraphics");
ObjC.import("Foundation");

// JXA's console.log writes to stderr and there is no built-in $.exit, so both
// are wired up by hand: results must reach stdout for the shell to grep, and
// the exit status is how app-drive.sh knows whether a click landed.
ObjC.bindFunction("exit", ["void", ["int"]]);

function write(handle, text) {
  handle.writeData(
    $.NSString.alloc.initWithUTF8String(text + "\n").dataUsingEncoding($.NSUTF8StringEncoding),
  );
}
function emit(text) {
  write($.NSFileHandle.fileHandleWithStandardOutput, text);
}
function warn(text) {
  write($.NSFileHandle.fileHandleWithStandardError, text);
}

var MAX_DEPTH = 24;

// Roles that represent something a user can act on. Used both to decide what
// `tree` is worth printing and what `click`/`focus` are allowed to match.
var ACTIONABLE = {
  AXButton: 1,
  AXCheckBox: 1,
  AXComboBox: 1,
  AXDisclosureTriangle: 1,
  AXIncrementor: 1,
  AXLink: 1,
  AXMenuButton: 1,
  AXMenuItem: 1,
  AXPopUpButton: 1,
  AXRadioButton: 1,
  AXSearchField: 1,
  AXSlider: 1,
  AXTabButton: 1,
  AXTextArea: 1,
  AXTextField: 1,
};

function env(name) {
  var value = $.NSProcessInfo.processInfo.environment.objectForKey(name);
  return value ? value.js : "";
}

function rawAttr(element, name) {
  var out = Ref();
  if ($.AXUIElementCopyAttributeValue(element, $(name), out) !== 0) return null;
  return out[0];
}

function strAttr(element, name) {
  var value = rawAttr(element, name);
  if (!value) return "";
  try {
    var unwrapped = ObjC.castRefToObject(value).js;
    return typeof unwrapped === "string" ? unwrapped : String(unwrapped);
  } catch (e) {
    return "";
  }
}

function children(element) {
  var kids = rawAttr(element, "AXChildren");
  return kids ? ObjC.castRefToObject(kids) : null;
}

// The label a human would use for this node: buttons carry AXTitle, links and
// aria-labelled controls carry AXDescription.
function labelOf(element) {
  return strAttr(element, "AXTitle") || strAttr(element, "AXDescription");
}

// What `click`/`find` match on. Plain text nodes have no label, so their content
// stands in for one.
function nameOf(element) {
  return labelOf(element) || strAttr(element, "AXValue");
}

function walk(element, depth, visit) {
  if (depth > MAX_DEPTH) return;
  var printed = visit(element, depth) ? 1 : 0;
  var kids = children(element);
  if (!kids) return;
  for (var i = 0; i < kids.count; i++) walk(kids.objectAtIndex(i), depth + printed, visit);
}

function rootWindow() {
  var pid = parseInt(env("AX_PID"), 10);
  if (!pid) {
    warn("ax-drive: AX_PID is required");
    return null;
  }
  var app = $.AXUIElementCreateApplication(pid);
  var windows = rawAttr(app, "AXWindows");
  if (!windows) {
    warn("ax-drive: no accessible windows for pid " + pid);
    return null;
  }
  var list = ObjC.castRefToObject(windows);
  if (list.count === 0) {
    warn("ax-drive: process " + pid + " has no window yet");
    return null;
  }
  return list.objectAtIndex(0);
}

function clamp(text) {
  var out = text.replace(/\s+/g, " ").trim();
  return out.length > 90 ? out.slice(0, 90) + "…" : out;
}

function describe(element, depth) {
  var role = strAttr(element, "AXRole").replace(/^AX/, "");
  var label = clamp(labelOf(element));
  var value = clamp(strAttr(element, "AXValue"));
  var indent = new Array(depth + 1).join("  ");
  // A labelled control shows both, so the current contents of a text field are
  // visible without a screenshot; an unlabelled text node shows its value alone.
  var text = label || value;
  if (label && value && value !== label) text = label + ' = "' + value + '"';
  return indent + "[" + role + "] " + text;
}

function commandTree(window) {
  var full = env("AX_FULL") === "1";
  var lines = [];
  walk(window, 0, function (element, depth) {
    var role = strAttr(element, "AXRole");
    if (!full && !ACTIONABLE[role] && !nameOf(element)) return false;
    lines.push(describe(element, depth));
    return true;
  });
  emit(lines.join("\n"));
  return 0;
}

// Match in tiers so an exact label always beats an incidental substring hit:
// exact, then case-insensitive exact, then case-insensitive substring.
function matches(window, role, target) {
  var tiers = [[], [], []];
  var wanted = target.toLowerCase();
  walk(window, 0, function (element) {
    var elementRole = strAttr(element, "AXRole");
    if (role) {
      if (elementRole !== role) return false;
    } else if (!ACTIONABLE[elementRole]) {
      return false;
    }
    var name = nameOf(element).replace(/\s+/g, " ").trim();
    if (!name) return false;
    var lower = name.toLowerCase();
    if (name === target) tiers[0].push([element, elementRole, name]);
    else if (lower === wanted) tiers[1].push([element, elementRole, name]);
    else if (lower.indexOf(wanted) !== -1) tiers[2].push([element, elementRole, name]);
    return false;
  });
  for (var i = 0; i < tiers.length; i++) if (tiers[i].length) return tiers[i];
  return [];
}

function resolve(window, role, target) {
  var found = matches(window, role, target);
  if (found.length === 0) {
    warn('ax-drive: no actionable element named "' + target + '"');
    warn("run `app-drive.sh tree` to see what is on screen");
    return null;
  }
  if (found.length === 1) return found[0];

  var index = env("AX_INDEX");
  if (index !== "") {
    var picked = found[parseInt(index, 10)];
    if (!picked) {
      warn("ax-drive: AX_INDEX " + index + " is out of range (" + found.length + " matches)");
      return null;
    }
    return picked;
  }
  warn('ax-drive: "' + target + '" is ambiguous - ' + found.length + " matches:");
  for (var i = 0; i < found.length; i++) {
    warn("  [" + i + "] " + found[i][1].replace(/^AX/, "") + " " + found[i][2]);
  }
  warn("re-run with --index N to pick one");
  return null;
}

function commandFind(window) {
  var hit = resolve(window, env("AX_ROLE"), env("AX_TARGET"));
  if (!hit) return 1;
  emit(hit[1].replace(/^AX/, "") + " " + hit[2]);
  return 0;
}

function commandClick(window) {
  var hit = resolve(window, env("AX_ROLE"), env("AX_TARGET"));
  if (!hit) return 1;
  var code = $.AXUIElementPerformAction(hit[0], $("AXPress"));
  if (code !== 0) {
    warn('ax-drive: AXPress on "' + hit[2] + '" failed (AXError ' + code + ")");
    return 1;
  }
  emit("clicked " + hit[1].replace(/^AX/, "") + " " + hit[2]);
  return 0;
}

// The CGWindowID lets screencapture grab this exact window even when another
// agent's instance overlaps it, which capturing a screen rectangle cannot do.
function commandWindowId() {
  var pid = parseInt(env("AX_PID"), 10);
  var raw = $.CGWindowListCopyWindowInfo(
    $.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements,
    $.kCGNullWindowID,
  );
  var info = ObjC.deepUnwrap(ObjC.castRefToObject(raw));
  for (var i = 0; i < info.length; i++) {
    // Layer 0 is a normal window; the tray icon and shadows live on other layers.
    if (info[i].kCGWindowOwnerPID === pid && info[i].kCGWindowLayer === 0) {
      emit(String(info[i].kCGWindowNumber));
      return 0;
    }
  }
  warn("ax-drive: no on-screen window for pid " + pid);
  return 1;
}

function main() {
  if (env("AX_CMD") === "windowid") return commandWindowId();
  var window = rootWindow();
  if (!window) return 1;
  var command = env("AX_CMD");
  if (command === "tree") return commandTree(window);
  if (command === "find") return commandFind(window);
  if (command === "click") return commandClick(window);
  warn("ax-drive: unknown AX_CMD " + command);
  return 1;
}

$.exit(main());
