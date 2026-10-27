#!/usr/bin/env node
// Launch an isolated OpenRize instance, click through its real webview UI, read
// the result back, and tear the instance down. Windows only.
//
// This is the Windows counterpart of scripts/app-drive.sh. The macOS script
// drives the app through the OS accessibility API; on Windows the webview is
// WebView2 (Chromium), which can expose the Chrome DevTools Protocol (CDP) on a
// local port when launched with `--remote-debugging-port`. Everything here goes
// through that port: the accessibility tree comes from CDP's Accessibility
// domain, clicks and keystrokes are real `Input.dispatch*` events, and
// screenshots are `Page.captureScreenshot`, which captures the webview only
// (never the desktop, which could show the user's private windows).
//
// It is plain ESM JavaScript with no dependencies (Node 22+ for the global
// WebSocket and fetch), so it runs with just `node`; it is `.mjs` rather than
// `.ts` like scripts/publish-r2 because nothing here needs the type-checking
// project that the publisher has.
//
// Sessions are namespaced by the checkout they were started from, so two agents
// in two worktrees never see or kill each other's sessions.
//
// See .agents/skills/drive-app-windows/SKILL.md for the workflow.
import { spawn, spawnSync } from "node:child_process";
import { createHash, randomInt } from "node:crypto";
import {
  closeSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { parseArgs } from "node:util";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const WORKSPACE_KEY = createHash("sha1").update(REPO_ROOT.toLowerCase()).digest("hex").slice(0, 10);
const SESSION_ROOT = join(tmpdir(), "openrize-app-drive", WORKSPACE_KEY);
const SHOT_DIR = join(tmpdir(), "openrize-dev-screenshot");
const TAURI_JS = join(REPO_ROOT, "node_modules", "@tauri-apps", "cli", "tauri.js");

// How long `start` waits for the app. A warm src-tauri/target gets there in
// seconds; the first cold `cargo build` of the dependency tree takes minutes.
const STARTUP_TIMEOUT = Number(process.env.OPENRIZE_DRIVE_TIMEOUT ?? 900);

// Every identifier this tool ever creates matches this, and nothing it deletes
// can be anything else. The real app's identifier (com.offlinestudios.openrize)
// does not match.
const IDENTIFIER_PREFIX = "com.offlinestudios.openrize.drive";
const IDENTIFIER_RE = /^com\.offlinestudios\.openrize\.drive\d+$/;

const USAGE = `usage: node scripts/app-drive-windows.mjs <command> [args]

  start                       launch an isolated instance, print its session id
  list                        list live sessions started from this checkout
  screenshot                  one-shot: start, screenshot, stop; print the PNG path
  targets [-s ID]             list the instance's webview windows (main, pulse, ...)
  tree   [-s ID] [--full]     print the window's accessibility tree
  find   [-s ID] <name>       resolve a control by its accessible name
  click  [-s ID] <name>       click a control (real mouse click at its centre)
  fill   [-s ID] <name> <txt> focus a field and replace its contents with text
  key    [-s ID] <keyspec>    send a keystroke, e.g. return, tab, ctrl+comma, ctrl+,
  text   [-s ID]              print the window's visible text (document.body.innerText)
  wait   [-s ID] <text> [sec] block until text appears in the page (default 15s)
  shot   [-s ID]              screenshot the webview, print the PNG path
  logs   [-s ID] [n]          tail the instance's vite/cargo log
  console [-s ID] [sec]       print console output and uncaught exceptions
  eval   [-s ID] <js>         evaluate a JS expression in the page (debugging)
  stop   [-s ID | --all]      kill the instance and delete its data

Options:
  -s, --session ID   target a specific session (default: the only live one)
  -t, --target STR   pick a webview by window label, title or URL substring
                     (default: the 'main' window)
  -r, --role ROLE    restrict matching to an accessibility role, e.g. button
  -i, --index N      pick the Nth match when a name is ambiguous
      --full         tree: include unnamed and static-text nodes
      --append       fill: keep the field's current text instead of replacing it
      --all          stop: every session started from this checkout`;

class DriveError extends Error {}

function die(message) {
  throw new DriveError(message);
}

const sleep = (ms) => new Promise((done) => setTimeout(done, ms));

// ---------------------------------------------------------------- session state

function sessionDir(id) {
  return join(SESSION_ROOT, id);
}

function readMeta(id) {
  try {
    return JSON.parse(readFileSync(join(sessionDir(id), "meta.json"), "utf8"));
  } catch {
    return null;
  }
}

function pidAlive(pid) {
  if (!pid) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    // EPERM means it exists but is not ours; ESRCH means it is gone.
    return error.code === "EPERM";
  }
}

function allSessionIds() {
  if (!existsSync(SESSION_ROOT)) return [];
  return readdirSync(SESSION_ROOT).filter((name) => /^d\d+$/.test(name));
}

function liveSessions() {
  return allSessionIds()
    .map(readMeta)
    .filter((meta) => meta && pidAlive(meta.pid));
}

function resolveSession(options) {
  const wanted = options.session ?? process.env.OPENRIZE_SESSION;
  if (wanted) {
    const meta = readMeta(wanted);
    if (!meta) die(`no such session: ${wanted}`);
    if (!pidAlive(meta.pid)) die(`session ${wanted} is no longer running`);
    return meta;
  }
  const live = liveSessions();
  if (live.length === 0) die("no live session - run 'node scripts/app-drive-windows.mjs start' first");
  if (live.length > 1) die(`several live sessions (${live.map((m) => m.id).join(" ")}) - pass -s ID`);
  return live[0];
}

// ---------------------------------------------------------------------- helpers

/** A port nothing listens on, on both loopback families (vite may bind either). */
function portFree(port) {
  const tryHost = (host) =>
    new Promise((done) => {
      const server = createServer();
      server.once("error", (error) =>
        // No IPv6 on this machine is fine; anything else (in use, excluded) is not.
        done(error.code === "EADDRNOTAVAIL" || error.code === "EAFNOSUPPORT"),
      );
      server.listen(port, host, () => server.close(() => done(true)));
    });
  return Promise.all([tryHost("127.0.0.1"), tryHost("::1")]).then((results) => results.every(Boolean));
}

async function pickPort(taken) {
  for (let attempt = 0; attempt < 60; attempt++) {
    const port = randomInt(20000, 40000);
    if (taken.has(port)) continue;
    if (existsSync(sessionDir(`d${port}`))) continue;
    if (await portFree(port)) {
      taken.add(port);
      return port;
    }
  }
  return die("could not find a free port");
}

function powershell(script) {
  const result = spawnSync(
    "powershell.exe",
    ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script],
    { encoding: "utf8", windowsHide: true },
  );
  return (result.stdout ?? "").trim();
}

function taskkillTree(pid) {
  if (!pid) return;
  spawnSync("taskkill", ["/T", "/F", "/PID", String(pid)], { stdio: "ignore", windowsHide: true });
}

/**
 * Processes whose command line mentions this session's identifier or debugging
 * port: the WebView2 browser/renderer/GPU processes (`--user-data-folder` is
 * under the identifier) and the vite server. Used to find orphans after the
 * tree kill, and to learn the app's own pid at start.
 */
function sessionProcesses(meta) {
  const needles = [meta.identifier, `remote-debugging-port=${meta.cdpPort}`, `--port ${meta.port} --strictPort`];
  const conditions = needles.map((needle) => `$_.CommandLine -like '*${needle}*'`).join(" -or ");
  const out = powershell(
    `Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -and $_.CommandLine -notlike "*Get-CimInstance*" -and (${conditions}) } | ` +
      "Select-Object ProcessId, ParentProcessId, CommandLine | ConvertTo-Json -Compress",
  );
  if (!out) return [];
  const parsed = JSON.parse(out);
  return Array.isArray(parsed) ? parsed : [parsed];
}

function removeDir(path) {
  rmSync(path, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 });
}

/** The directories Tauri creates for an identifier. Refuses anything that is not ours. */
function identifierDirs(identifier) {
  if (!IDENTIFIER_RE.test(identifier)) die(`refusing to touch data for identifier ${identifier}`);
  const dirs = [];
  if (process.env.APPDATA) dirs.push(join(process.env.APPDATA, identifier));
  if (process.env.LOCALAPPDATA) dirs.push(join(process.env.LOCALAPPDATA, identifier));
  return dirs.filter((dir) => basename(dir) === identifier);
}

// ------------------------------------------------------------------- CDP client

/** A minimal Chrome DevTools Protocol connection to one page target. */
class Cdp {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 0;
    this.pending = new Map();
    this.listeners = [];
    socket.onmessage = (event) => {
      const message = JSON.parse(event.data);
      const waiting = message.id === undefined ? undefined : this.pending.get(message.id);
      if (waiting) {
        this.pending.delete(message.id);
        if (message.error) waiting.reject(new DriveError(`${waiting.method}: ${message.error.message}`));
        else waiting.resolve(message.result ?? {});
      } else {
        for (const listener of this.listeners) listener(message);
      }
    };
    socket.onclose = () => {
      for (const waiting of this.pending.values()) waiting.reject(new DriveError("the webview closed the connection"));
      this.pending.clear();
    };
  }

  static connect(url) {
    return new Promise((done, fail) => {
      const socket = new WebSocket(url);
      socket.onopen = () => done(new Cdp(socket));
      socket.onerror = () => fail(new DriveError(`could not connect to ${url}`));
    });
  }

  send(method, params = {}, timeoutMs = 30000) {
    return new Promise((resolvePromise, reject) => {
      const id = ++this.nextId;
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new DriveError(`${method} timed out after ${timeoutMs}ms`));
      }, timeoutMs);
      this.pending.set(id, {
        method,
        resolve: (value) => {
          clearTimeout(timer);
          resolvePromise(value);
        },
        reject: (error) => {
          clearTimeout(timer);
          reject(error);
        },
      });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  on(listener) {
    this.listeners.push(listener);
  }

  async evaluate(expression) {
    const { result, exceptionDetails } = await this.send("Runtime.evaluate", {
      expression,
      awaitPromise: true,
      returnByValue: true,
    });
    if (exceptionDetails) {
      die(`page threw: ${exceptionDetails.exception?.description ?? exceptionDetails.text}`);
    }
    return result.value;
  }

  /** Close and wait for the socket to finish closing (exiting mid-close trips a libuv assertion on Windows). */
  close() {
    return new Promise((done) => {
      const timer = setTimeout(done, 1000);
      this.socket.onclose = () => {
        clearTimeout(timer);
        done();
      };
      this.socket.close();
    });
  }
}

async function cdpTargets(meta) {
  let response;
  try {
    response = await fetch(`http://127.0.0.1:${meta.cdpPort}/json`, { signal: AbortSignal.timeout(3000) });
  } catch {
    return die(`the webview's debugging port ${meta.cdpPort} is not answering (still starting? try 'logs')`);
  }
  return (await response.json()).filter((target) => target.type === "page");
}

/** The window label Tauri gave a page ("main", "pulse", ...), or "" if not Tauri yet. */
async function windowLabel(target) {
  let cdp;
  try {
    cdp = await Cdp.connect(target.webSocketDebuggerUrl);
    return (await cdp.evaluate("window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? ''")) || "";
  } catch {
    return "";
  } finally {
    await cdp?.close();
  }
}

/** Pick the page to drive: `--target` substring, else the window labelled "main". */
async function pickTarget(meta, options) {
  const pages = await cdpTargets(meta);
  if (pages.length === 0) die("the instance has no webview page yet");
  const labelled = await Promise.all(pages.map(async (page) => ({ page, label: await windowLabel(page) })));
  const wanted = options.target?.toLowerCase();
  if (wanted) {
    const hits = labelled.filter(
      ({ page, label }) =>
        label.toLowerCase() === wanted ||
        page.title.toLowerCase().includes(wanted) ||
        page.url.toLowerCase().includes(wanted),
    );
    if (hits.length === 0) die(`no webview matches "${options.target}" (try 'targets')`);
    return hits[0].page;
  }
  return (labelled.find(({ label }) => label === "main") ?? labelled[0]).page;
}

async function withPage(options, action) {
  const meta = resolveSession(options);
  const target = await pickTarget(meta, options);
  const cdp = await Cdp.connect(target.webSocketDebuggerUrl);
  try {
    // The window is rarely focused while an agent works; without this Chromium
    // treats the page as backgrounded and withholds focus-dependent behaviour.
    await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true }).catch(() => {});
    return await action(cdp, meta);
  } finally {
    await cdp.close();
  }
}

// ------------------------------------------------------------ accessibility tree

// Roles a user can act on: what `find`/`click`/`fill` match first, and what
// `tree` always prints. CDP reports Chromium's role strings, compared lowercased.
const ACTIONABLE = new Set([
  "button",
  "checkbox",
  "combobox",
  "comboboxselect",
  "disclosuretriangle",
  "link",
  "listboxoption",
  "menuitem",
  "menuitemcheckbox",
  "menuitemradio",
  "option",
  "popupbutton",
  "radio",
  "searchbox",
  "slider",
  "spinbutton",
  "switch",
  "tab",
  "textbox",
  "treeitem",
]);
const TEXT_FIELDS = new Set(["textbox", "searchbox", "combobox", "spinbutton"]);
// Structural noise: never printed (except by --full), and never matched by name.
const NOISE = new Set(["generic", "none", "presentation", "inlinetextbox", "linebreak", "rootwebarea"]);

async function axNodes(cdp) {
  await cdp.send("Accessibility.enable");
  // Chromium builds the tree lazily after enable; the first read can be a stub.
  let nodes = [];
  for (let attempt = 0; attempt < 10; attempt++) {
    ({ nodes } = await cdp.send("Accessibility.getFullAXTree"));
    if (nodes.length > 3) break;
    await sleep(300);
  }
  return nodes;
}

const roleOf = (node) => String(node.role?.value ?? "");
const nameOf = (node) => String(node.name?.value ?? "");
const valueOf = (node) => (node.value?.value === undefined ? "" : String(node.value.value));
const propOf = (node, name) => node.properties?.find((property) => property.name === name)?.value?.value;

/** Depth-first walk of the non-ignored nodes, yielding each with its depth. */
function* walk(nodes) {
  const byId = new Map(nodes.map((node) => [node.nodeId, node]));
  function* visit(node, depth) {
    if (!node.ignored) yield { node, depth };
    for (const childId of node.childIds ?? []) {
      const child = byId.get(childId);
      if (child) yield* visit(child, node.ignored ? depth : depth + 1);
    }
  }
  const root = nodes.find((node) => !node.parentId) ?? nodes[0];
  if (root) yield* visit(root, 0);
}

function describe(node) {
  const role = roleOf(node);
  const label = role.charAt(0).toUpperCase() + role.slice(1);
  let line = `[${label}]`;
  if (nameOf(node)) line += ` ${nameOf(node)}`;
  if (TEXT_FIELDS.has(role.toLowerCase()) || valueOf(node)) line += ` = ${JSON.stringify(valueOf(node))}`;
  const states = [];
  if (propOf(node, "disabled")) states.push("disabled");
  const checked = propOf(node, "checked");
  if (checked === "true") states.push("checked");
  if (checked === "mixed") states.push("mixed");
  if (propOf(node, "selected")) states.push("selected");
  if (propOf(node, "expanded") === true) states.push("expanded");
  if (propOf(node, "focused")) states.push("focused");
  return states.length > 0 ? `${line} (${states.join(", ")})` : line;
}

function renderTree(nodes, full) {
  const lines = [];
  // Indent by printed ancestors only, so skipped wrapper nodes do not widen the tree.
  const printed = [];
  for (const { node, depth } of walk(nodes)) {
    while (printed.length > 0 && printed[printed.length - 1] >= depth) printed.pop();
    const role = roleOf(node).toLowerCase();
    const named = nameOf(node) !== "";
    const show = full
      ? role !== "inlinetextbox"
      : ACTIONABLE.has(role) || role === "rootwebarea" || (named && !NOISE.has(role) && role !== "statictext");
    if (show) {
      lines.push(`${"  ".repeat(printed.length)}${describe(node)}`);
      printed.push(depth);
    }
  }
  return lines.join("\n");
}

/** Candidates for a name: actionable roles first, any named node as a fallback. */
function matches(nodes, name, options) {
  const role = options.role?.toLowerCase();
  const wanted = name.toLowerCase();
  const tiers = (pool) => {
    const exact = pool.filter((node) => nameOf(node) === name);
    if (exact.length) return exact;
    const folded = pool.filter((node) => nameOf(node).toLowerCase() === wanted);
    if (folded.length) return folded;
    return pool.filter((node) => nameOf(node).toLowerCase().includes(wanted));
  };
  const all = [...walk(nodes)].map(({ node }) => node).filter((node) => nameOf(node) !== "");
  const inRole = (pool) =>
    pool.filter((node) => {
      const nodeRole = roleOf(node).toLowerCase();
      return (!role || nodeRole === role) && (!options.roles || options.roles.has(nodeRole));
    });
  const actionable = tiers(inRole(all.filter((node) => ACTIONABLE.has(roleOf(node).toLowerCase()))));
  if (actionable.length > 0) return actionable;
  return tiers(inRole(all.filter((node) => !NOISE.has(roleOf(node).toLowerCase()))));
}

function pickMatch(nodes, name, options) {
  const found = matches(nodes, name, options);
  if (found.length === 0) die(`nothing on screen is named "${name}" (try 'tree')`);
  if (options.index !== undefined) {
    const chosen = found[Number(options.index)];
    if (!chosen) die(`only ${found.length} match(es) for "${name}"; -i ${options.index} is out of range`);
    return chosen;
  }
  if (found.length > 1) {
    const list = found.map((node, index) => `  -i ${index}  ${describe(node)}`).join("\n");
    die(`"${name}" is ambiguous (${found.length} matches); pass -i N or -r ROLE:\n${list}`);
  }
  return found[0];
}

// --------------------------------------------------------------- input dispatch

/** Scroll a node into view and return the viewport point to click, checking nothing covers it. */
async function locate(cdp, node) {
  if (!node.backendDOMNodeId) die(`[${roleOf(node)}] ${nameOf(node)} has no DOM node to act on`);
  const { object } = await cdp.send("DOM.resolveNode", { backendNodeId: node.backendDOMNodeId });
  const { result, exceptionDetails } = await cdp.send("Runtime.callFunctionOn", {
    objectId: object.objectId,
    returnByValue: true,
    functionDeclaration: `function () {
      const el = this.nodeType === 1 ? this : this.parentElement;
      // "nearest", not "center": centering also scrolls the app's
      // overflow-hidden root, which shifts the whole page off the window.
      el.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
      const r = el.getBoundingClientRect();
      const x = r.x + r.width / 2;
      const y = r.y + r.height / 2;
      const hit = document.elementFromPoint(x, y);
      return { x, y, width: r.width, height: r.height,
        covered: !hit || !(el.contains(hit) || hit.contains(el)),
        blocker: hit ? hit.tagName.toLowerCase() + (hit.className ? "." + String(hit.className).split(" ")[0] : "") : "nothing" };
    }`,
  });
  if (exceptionDetails || !result.value) die(`could not locate [${roleOf(node)}] ${nameOf(node)}`);
  const point = result.value;
  if (point.width === 0 || point.height === 0) die(`"${nameOf(node)}" has no size on screen`);
  if (point.covered) die(`"${nameOf(node)}" is covered by <${point.blocker}> at its centre`);
  return point;
}

async function mouseClick(cdp, { x, y }) {
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y });
  await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", buttons: 1, clickCount: 1 });
  await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", buttons: 0, clickCount: 1 });
}

// Key names -> CDP key description. `vk` is the Windows virtual-key code.
const NAMED_KEYS = {
  return: { key: "Enter", code: "Enter", vk: 13, text: "\r" },
  enter: { key: "Enter", code: "Enter", vk: 13, text: "\r" },
  tab: { key: "Tab", code: "Tab", vk: 9 },
  space: { key: " ", code: "Space", vk: 32, text: " " },
  escape: { key: "Escape", code: "Escape", vk: 27 },
  esc: { key: "Escape", code: "Escape", vk: 27 },
  backspace: { key: "Backspace", code: "Backspace", vk: 8 },
  delete: { key: "Delete", code: "Delete", vk: 46 },
  insert: { key: "Insert", code: "Insert", vk: 45 },
  left: { key: "ArrowLeft", code: "ArrowLeft", vk: 37 },
  up: { key: "ArrowUp", code: "ArrowUp", vk: 38 },
  right: { key: "ArrowRight", code: "ArrowRight", vk: 39 },
  down: { key: "ArrowDown", code: "ArrowDown", vk: 40 },
  home: { key: "Home", code: "Home", vk: 36 },
  end: { key: "End", code: "End", vk: 35 },
  pageup: { key: "PageUp", code: "PageUp", vk: 33 },
  pagedown: { key: "PageDown", code: "PageDown", vk: 34 },
  comma: { key: ",", code: "Comma", vk: 188, text: "," },
  period: { key: ".", code: "Period", vk: 190, text: "." },
  slash: { key: "/", code: "Slash", vk: 191, text: "/" },
  minus: { key: "-", code: "Minus", vk: 189, text: "-" },
  equals: { key: "=", code: "Equal", vk: 187, text: "=" },
  plus: { key: "+", code: "Equal", vk: 187, text: "+" },
};
const PUNCTUATION = {
  ",": ["Comma", 188],
  ".": ["Period", 190],
  "/": ["Slash", 191],
  ";": ["Semicolon", 186],
  "'": ["Quote", 222],
  "[": ["BracketLeft", 219],
  "]": ["BracketRight", 221],
  "\\": ["Backslash", 220],
  "-": ["Minus", 189],
  "=": ["Equal", 187],
  "`": ["Backquote", 192],
};
// CDP modifier bitmask.
const MODIFIERS = { alt: 1, opt: 1, option: 1, ctrl: 2, control: 2, cmd: 4, command: 4, meta: 4, win: 4, shift: 8 };

/** Parse "ctrl+shift+tab", "ctrl+,", "ctrl++", "f5", "a" into a CDP key description. */
function parseKeySpec(spec) {
  let rest = spec;
  let modifiers = 0;
  while (rest.length > 1 && rest.includes("+")) {
    const prefix = rest.slice(0, rest.indexOf("+"));
    const bit = MODIFIERS[prefix.toLowerCase()];
    if (!bit) die(`unknown modifier "${prefix}" in "${spec}"`);
    modifiers |= bit;
    rest = rest.slice(rest.indexOf("+") + 1);
  }
  const lower = rest.toLowerCase();
  let description = NAMED_KEYS[lower];
  if (!description && /^f([1-9]|1[0-2])$/.test(lower)) {
    description = { key: rest.toUpperCase(), code: rest.toUpperCase(), vk: 111 + Number(lower.slice(1)) };
  }
  if (!description && [...rest].length === 1) {
    const char = rest;
    const upper = char.toUpperCase();
    const shifted = (modifiers & 8) !== 0;
    if (/[a-z]/i.test(char)) {
      description = { key: shifted ? upper : char.toLowerCase(), code: `Key${upper}`, vk: upper.charCodeAt(0) };
    } else if (/[0-9]/.test(char)) {
      description = { key: char, code: `Digit${char}`, vk: char.charCodeAt(0) };
    } else if (PUNCTUATION[char]) {
      description = { key: char, code: PUNCTUATION[char][0], vk: PUNCTUATION[char][1] };
    } else {
      description = { key: char, code: "", vk: 0 };
    }
    description.text = description.key;
  }
  if (!description) die(`unknown key "${rest}" in "${spec}"`);
  // A shortcut (ctrl/alt/meta held) types no character, so drop the text.
  const text = modifiers & 7 ? undefined : description.text;
  return { ...description, text, modifiers };
}

async function pressKey(cdp, spec) {
  const { key, code, vk, text, modifiers } = parseKeySpec(spec);
  const base = { key, code, windowsVirtualKeyCode: vk, nativeVirtualKeyCode: vk, modifiers };
  await cdp.send("Input.dispatchKeyEvent", { type: text ? "keyDown" : "rawKeyDown", ...base, text, unmodifiedText: text });
  await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", ...base });
}

// --------------------------------------------------------------------- commands

async function startSession() {
  if (process.platform !== "win32") die("app-drive-windows.mjs only works on Windows (macOS: scripts/app-drive.sh)");
  if (!existsSync(TAURI_JS)) die("dependencies are not installed - run 'pnpm install' first");
  mkdirSync(SESSION_ROOT, { recursive: true });

  // Windows cannot overwrite a running .exe, and every session embeds its own
  // identifier in the binary, so a second session in this checkout would fail to
  // relink src-tauri/target/debug/openrize.exe. Separate worktrees are separate
  // checkouts with their own target dirs and run side by side.
  const running = liveSessions();
  if (running.length > 0) {
    die(`session ${running[0].id} is still running from this checkout and holds src-tauri/target/debug/openrize.exe; stop it first (or use another worktree)`);
  }
  const taken = new Set(liveSessions().flatMap((meta) => [meta.port, meta.cdpPort]));
  const port = await pickPort(taken);
  const cdpPort = await pickPort(taken);
  const id = `d${port}`;
  const identifier = `${IDENTIFIER_PREFIX}${port}`;
  const dir = sessionDir(id);
  removeDir(dir);
  mkdirSync(join(dir, "config"), { recursive: true });
  mkdirSync(join(dir, "shots"), { recursive: true });

  // Isolation, both parts required:
  //   identifier      -> Tauri's app_data_dir / app_local_data_dir and the
  //                      WebView2 user-data folder, i.e. the activity database
  //   XDG_CONFIG_HOME -> where settings.json lives, which is not identifier-scoped
  //                      and would otherwise be the real user's settings
  const overrides = JSON.stringify({
    identifier,
    build: {
      beforeDevCommand: `pnpm exec vite --port ${port} --strictPort`,
      devUrl: `http://localhost:${port}`,
    },
  });
  const cargoBin = join(homedir(), ".cargo", "bin");
  const env = {
    ...process.env,
    XDG_CONFIG_HOME: join(dir, "config"),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${cdpPort}`,
    PATH: existsSync(cargoBin) ? `${cargoBin};${process.env.PATH ?? process.env.Path ?? ""}` : (process.env.PATH ?? process.env.Path),
  };
  // The tauri CLI is run through node directly: `pnpm tauri` goes via bash and
  // .cmd shims, and a JSON argument does not survive cmd.exe quoting.
  const logPath = join(dir, "dev.log");
  const log = openSync(logPath, "w");
  console.error(`==> starting isolated instance ${id} on port ${port} (webview debug port ${cdpPort})`);
  const child = spawn(process.execPath, [TAURI_JS, "dev", "--no-watch", "-c", overrides], {
    cwd: REPO_ROOT,
    env,
    detached: true,
    stdio: ["ignore", log, log],
    windowsHide: true,
  });
  child.unref();
  closeSync(log);

  const [appData, localData] = identifierDirs(identifier);
  const meta = {
    id,
    port,
    cdpPort,
    pid: child.pid,
    appPid: null,
    identifier,
    appData,
    localData,
    configHome: join(dir, "config"),
    repoRoot: REPO_ROOT,
  };
  writeFileSync(join(dir, "meta.json"), JSON.stringify(meta, null, 2));

  try {
    await waitUntilDrivable(meta, logPath);
  } catch (error) {
    stopSession(meta);
    throw error;
  }
  // The app's own pid is the parent of the WebView2 browser process that owns
  // our debugging port. Recorded so stop can still reach it if the tree breaks.
  const browser = sessionProcesses(meta).find(
    (proc) => String(proc.CommandLine).includes(`remote-debugging-port=${cdpPort}`) && !String(proc.CommandLine).includes("--type="),
  );
  meta.appPid = browser?.ParentProcessId ?? null;
  writeFileSync(join(dir, "meta.json"), JSON.stringify(meta, null, 2));
  return meta;
}

async function waitUntilDrivable(meta, logPath) {
  const deadline = Date.now() + STARTUP_TIMEOUT * 1000;
  const tail = () => readFileSync(logPath, "utf8").split("\n").slice(-20).join("\n");
  while (Date.now() < deadline) {
    if (!pidAlive(meta.pid)) die(`the dev process exited before the app started (log: ${logPath})\n--- last 20 log lines ---\n${tail()}`);
    try {
      const pages = await cdpTargets(meta);
      if (pages.length > 0) {
        const cdp = await Cdp.connect((await pickTarget(meta, {})).webSocketDebuggerUrl);
        try {
          // The target exists before React has painted; wait for real controls.
          const ready = renderTree(await axNodes(cdp), false).includes("[Button]");
          if (ready) return;
        } finally {
          await cdp.close();
        }
      }
    } catch {
      // Not up yet; keep polling.
    }
    await sleep(1000);
  }
  return die(`app never became drivable within ${STARTUP_TIMEOUT}s (log: ${logPath})\n--- last 20 log lines ---\n${tail()}`);
}

function stopSession(meta) {
  taskkillTree(meta.pid);
  if (meta.appPid && pidAlive(meta.appPid)) taskkillTree(meta.appPid);
  // Orphans whose parents died first are out of reach of /T: find them by
  // their command line instead, which carries this session's identifier/ports.
  for (let sweep = 0; sweep < 3; sweep++) {
    const leftovers = sessionProcesses(meta).filter((proc) => proc.ProcessId !== process.pid);
    if (leftovers.length === 0) break;
    for (const proc of leftovers) taskkillTree(proc.ProcessId);
  }
  for (const dir of identifierDirs(meta.identifier)) removeDir(dir);
  removeDir(sessionDir(meta.id));
}

async function cmdStart() {
  const meta = await startSession();
  console.log(`session=${meta.id} port=${meta.port} cdp=${meta.cdpPort} pid=${meta.pid} identifier=${meta.identifier}`);
  console.log(`config=${meta.configHome} app-data=${meta.appData} local-data=${meta.localData}`);
}

function cmdList() {
  const live = liveSessions();
  if (live.length === 0) console.log(`no live sessions for ${REPO_ROOT}`);
  for (const meta of live) {
    console.log(`${meta.id}  port=${meta.port}  cdp=${meta.cdpPort}  pid=${meta.pid}  identifier=${meta.identifier}`);
  }
  // A window closed by hand ends the app without `stop`, which leaves its data.
  const dead = allSessionIds().filter((id) => !live.some((meta) => meta.id === id));
  if (dead.length > 0) console.log(`ended without stop (data left behind): ${dead.join(" ")} - run 'stop --all'`);
}

async function cmdScreenshot(options) {
  const meta = await startSession();
  try {
    const path = await withPage({ ...options, session: meta.id }, (cdp) => captureTo(cdp, SHOT_DIR));
    console.log(path);
  } finally {
    stopSession(meta);
  }
}

async function cmdTargets(options) {
  const meta = resolveSession(options);
  for (const page of await cdpTargets(meta)) {
    console.log(`${(await windowLabel(page)) || "-"}\t${page.title}\t${page.url}`);
  }
}

async function cmdTree(options) {
  await withPage(options, async (cdp) => console.log(renderTree(await axNodes(cdp), Boolean(options.full))));
}

async function cmdFind(options, [name]) {
  if (!name) die("find needs a name");
  await withPage(options, async (cdp) => {
    const node = pickMatch(await axNodes(cdp), name, options);
    const point = await locate(cdp, node);
    console.log(`${describe(node)} at (${Math.round(point.x)}, ${Math.round(point.y)})`);
  });
}

async function cmdClick(options, [name]) {
  if (!name) die("click needs a name");
  await withPage(options, async (cdp) => {
    const node = pickMatch(await axNodes(cdp), name, options);
    if (propOf(node, "disabled")) die(`${describe(node)} is disabled`);
    await mouseClick(cdp, await locate(cdp, node));
    console.log(`clicked ${describe(node)}`);
  });
}

async function cmdFill(options, [name, text]) {
  if (name === undefined || text === undefined) die("fill needs a field name and text");
  await withPage(options, async (cdp) => {
    const nodes = await axNodes(cdp);
    // Unless told otherwise, only text-entry roles are candidates.
    const node = pickMatch(nodes, name, options.role ? options : { ...options, roles: TEXT_FIELDS });
    if (propOf(node, "disabled")) die(`${describe(node)} is disabled`);
    await mouseClick(cdp, await locate(cdp, node));
    // Clicking focuses the field exactly as a user would. Writing .value would
    // bypass React's onChange, so select the old text and type over it instead.
    if (!options.append) {
      await cdp.evaluate(
        `(() => { const el = document.activeElement; if (el && typeof el.select === "function") el.select(); else document.execCommand("selectAll"); })()`,
      );
    }
    if (text === "") await cdp.evaluate(`document.execCommand("delete")`);
    else await cdp.send("Input.insertText", { text });
    console.log(`typed into ${name}`);
  });
}

async function cmdKey(options, [spec]) {
  if (!spec) die("key needs a keyspec");
  await withPage(options, async (cdp) => {
    await pressKey(cdp, spec);
    console.log(`sent ${spec}`);
  });
}

async function cmdText(options) {
  await withPage(options, async (cdp) => console.log(await cdp.evaluate("document.body.innerText")));
}

async function cmdWait(options, [needle, seconds]) {
  if (!needle) die("wait needs text to look for");
  const limit = Number(seconds ?? 15);
  await withPage(options, async (cdp) => {
    const wanted = needle.toLowerCase();
    const deadline = Date.now() + limit * 1000;
    do {
      const visible = String(await cdp.evaluate("document.body.innerText")).toLowerCase();
      // Accessible names (aria-labels) never appear in innerText, so check them too.
      const names = (await axNodes(cdp)).map((node) => `${nameOf(node)}\n${valueOf(node)}`).join("\n").toLowerCase();
      if (visible.includes(wanted) || names.includes(wanted)) {
        console.log(`found: ${needle}`);
        return;
      }
      await sleep(500);
    } while (Date.now() < deadline);
    die(`timed out after ${limit}s waiting for: ${needle}`);
  });
}

/** Save a webview-only PNG into `dir` and return its path. */
async function captureTo(cdp, dir) {
  mkdirSync(dir, { recursive: true });
  const { data } = await cdp.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
  const out = join(dir, `${Date.now()}-${randomInt(1000, 9999)}.png`);
  writeFileSync(out, Buffer.from(data, "base64"));
  return out;
}

async function cmdShot(options) {
  await withPage(options, async (cdp, meta) => console.log(await captureTo(cdp, join(sessionDir(meta.id), "shots"))));
}

function cmdLogs(options, [count]) {
  const meta = resolveSession(options);
  const lines = readFileSync(join(sessionDir(meta.id), "dev.log"), "utf8").split("\n");
  console.log(lines.slice(-Number(count ?? 40)).join("\n"));
}

async function cmdConsole(options, [seconds]) {
  await withPage(options, async (cdp) => {
    // Enabling the domains replays what the page logged before we connected.
    cdp.on((message) => {
      if (message.method === "Runtime.consoleAPICalled") {
        const { type, args } = message.params;
        console.log(`[console.${type}] ${args.map((arg) => arg.value ?? arg.description ?? arg.type).join(" ")}`);
      } else if (message.method === "Runtime.exceptionThrown") {
        const { text, exception } = message.params.exceptionDetails;
        console.log(`[exception] ${exception?.description ?? text}`);
      } else if (message.method === "Log.entryAdded") {
        console.log(`[log.${message.params.entry.level}] ${message.params.entry.text}`);
      }
    });
    await cdp.send("Runtime.enable");
    await cdp.send("Log.enable");
    await sleep(Number(seconds ?? 1) * 1000);
  });
}

async function cmdEval(options, [expression]) {
  if (!expression) die("eval needs a JavaScript expression");
  await withPage(options, async (cdp) => {
    const value = await cdp.evaluate(expression);
    console.log(typeof value === "string" ? value : JSON.stringify(value, null, 2));
  });
}

async function cmdStop(options) {
  let sessions;
  if (options.all) {
    // Includes sessions whose process already died, so their data still goes.
    sessions = allSessionIds().map(readMeta).filter(Boolean);
    if (sessions.length === 0) {
      console.log(`no sessions for ${REPO_ROOT}`);
      return;
    }
  } else {
    sessions = [resolveSession(options)];
  }
  for (const meta of sessions) {
    stopSession(meta);
    console.log(`stopped ${meta.id}`);
  }
}

// ------------------------------------------------------------------ entry point

const COMMANDS = {
  start: cmdStart,
  list: cmdList,
  screenshot: cmdScreenshot,
  targets: cmdTargets,
  tree: cmdTree,
  find: cmdFind,
  click: cmdClick,
  fill: cmdFill,
  key: cmdKey,
  text: cmdText,
  wait: cmdWait,
  shot: cmdShot,
  logs: cmdLogs,
  console: cmdConsole,
  eval: cmdEval,
  stop: cmdStop,
};

async function main() {
  const { values, positionals } = parseArgs({
    allowPositionals: true,
    options: {
      session: { type: "string", short: "s" },
      target: { type: "string", short: "t" },
      role: { type: "string", short: "r" },
      index: { type: "string", short: "i" },
      full: { type: "boolean" },
      append: { type: "boolean" },
      all: { type: "boolean" },
      help: { type: "boolean", short: "h" },
    },
  });
  const [command, ...args] = positionals;
  if (values.help || command === "help" || command === undefined) {
    console.log(USAGE);
    process.exit(command === undefined && !values.help ? 1 : 0);
  }
  const run = COMMANDS[command];
  if (!run) {
    console.error(USAGE);
    process.exit(1);
  }
  await run(values, args);
}

main().then(
  async () => {
    await sleep(100);
    process.exit(0);
  },
  (error) => {
    console.error(error instanceof DriveError ? `app-drive-windows: ${error.message}` : error);
    process.exit(1);
  },
);
