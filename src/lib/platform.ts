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
