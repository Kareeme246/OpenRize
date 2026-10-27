/**
 * Platform conventions the UI follows: which modifier shortcuts use, how
 * they are written, and what the OS calls its own places.
 *
 * Every platform-dependent value is a `Record<Platform, T>`, so supporting a
 * new platform is a type error at each one until it is spelled out.
 */

export type Platform = "macos" | "windows" | "linux";

function detect(): Platform {
  const userAgent = navigator.userAgent;
  if (userAgent.includes("Mac")) return "macos";
  if (userAgent.includes("Windows")) return "windows";
  return "linux";
}

export const PLATFORM: Platform = detect();

function byPlatform<T>(values: Record<Platform, T>): T {
  return values[PLATFORM];
}

export const OS_NAME = byPlatform({
  macos: "macOS",
  windows: "Windows",
  linux: "Linux",
});

/** Where the app's icon lives, as the OS names it. */
export const TRAY_PLACE = byPlatform({
  macos: "menu bar",
  windows: "system tray",
  linux: "system tray",
});

export const TRAY_ICON = byPlatform({
  macos: "Menu bar icon",
  windows: "Tray icon",
  linux: "Tray icon",
});

export const FILE_MANAGER = byPlatform({
  macos: "Finder",
  windows: "File Explorer",
  linux: "Files",
});

export const THIS_COMPUTER = byPlatform({
  macos: "this Mac",
  windows: "this PC",
  linux: "this computer",
});

/**
 * Whether the OS draws its own window caption (title and window buttons)
 * above the webview. macOS overlays its title bar on the webview instead
 * (`titleBarStyle: "Overlay"` in tauri.conf.json), so the app's own bar is
 * the title bar there.
 */
export const HAS_NATIVE_CAPTION = byPlatform({
  macos: false,
  windows: true,
  linux: true,
});

/** The Battery & Energy monitor. Windows has no per-process energy counter. */
export const HAS_ENERGY_MONITOR = byPlatform({
  macos: true,
  windows: false,
  linux: true,
});

/**
 * Whether this platform's build is expected to ship the on-device ML sidecar.
 * Only the first guess, before the AI status has loaded: after that the
 * status's `sidecar` says what is actually there.
 */
export const EXPECTS_ON_DEVICE_ML = byPlatform({
  macos: true,
  windows: false,
  linux: false,
});

/** The OS setting that throttles background work to save battery. */
export const POWER_SAVING_MODE = byPlatform({
  macos: "Low Power Mode",
  windows: "Battery Saver",
  linux: "power saver",
});

/** The modifier app shortcuts use (and not the other one). */
export function hasPrimaryModifier(event: KeyboardEvent): boolean {
  return byPlatform({
    macos: event.metaKey && !event.ctrlKey,
    windows: event.ctrlKey && !event.metaKey,
    linux: event.ctrlKey && !event.metaKey,
  });
}

/** Keys with a symbol, which macOS always writes and a compact label uses. */
const KEY_SYMBOLS: Record<string, string> = { Enter: "↵", Backspace: "⌫" };

/**
 * A shortcut as the platform writes it: "⌘⇧↵" or "Ctrl+Shift+Enter".
 * `compact` writes a key as its symbol where it has one ("Ctrl+⌫"), for a
 * label inside a button; tooltips and prose keep the full name.
 */
export function shortcutLabel(
  key: string,
  {
    shift = false,
    compact = false,
  }: { shift?: boolean; compact?: boolean } = {},
): string {
  const symbol = KEY_SYMBOLS[key] ?? key;
  const ctrl = [
    "Ctrl",
    ...(shift ? ["Shift"] : []),
    compact ? symbol : key,
  ].join("+");
  return byPlatform({
    macos: `⌘${shift ? "⇧" : ""}${symbol}`,
    windows: ctrl,
    linux: ctrl,
  });
}
