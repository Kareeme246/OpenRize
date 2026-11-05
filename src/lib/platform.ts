/**
 * Platform conventions the UI follows; each is a `Record<Platform, T>`, so a
 * new platform is a type error until spelled out.
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

export const TRASH = byPlatform({
  macos: "Trash",
  windows: "Recycle Bin",
  linux: "Trash",
});

/**
 * Whether the OS draws its own caption above the webview; macOS overlays its
 * title bar and Windows has it removed (src-tauri/src/caption.rs).
 */
export const HAS_NATIVE_CAPTION = byPlatform({
  macos: false,
  windows: false,
  linux: true,
});

/**
 * Whether the app draws minimize/maximize/close itself; macOS draws traffic
 * lights over the webview.
 */
export const DRAWS_CAPTION_BUTTONS = byPlatform({
  macos: false,
  windows: true,
  linux: false,
});

/**
 * Whether right-clicking the top bar opens the system window menu, where the
 * app's bar replaces a removed caption.
 */
export const BAR_OPENS_WINDOW_MENU = byPlatform({
  macos: false,
  windows: true,
  linux: false,
});

/**
 * Whether the window's own theme follows the app's. It sets the scheme of
 * OS-drawn menus, so Windows keeps it even without a caption.
 */
export const SYNCS_WINDOW_THEME = byPlatform({
  macos: false,
  windows: true,
  linux: true,
});

/**
 * Left padding of the top bar; on macOS it clears the overlaid traffic lights.
 */
export const TOP_BAR_PADDING_LEFT = byPlatform({
  macos: "pl-[84px]",
  windows: "pl-3",
  linux: "pl-3",
});

/**
 * Right padding of the top bar; Windows' caption buttons run to the window
 * edge.
 */
export const TOP_BAR_PADDING_RIGHT = byPlatform({
  macos: "pr-3",
  windows: "pr-0",
  linux: "pr-3",
});

export const HAS_PERMISSIONS = byPlatform({
  macos: true,
  windows: false,
  linux: false,
});

/** Windows has no per-process energy counter. */
export const HAS_ENERGY_MONITOR = byPlatform({
  macos: true,
  windows: false,
  linux: true,
});

/**
 * Whether the build is expected to ship the ML sidecar; only a first guess
 * until the AI status loads.
 */
export const EXPECTS_ON_DEVICE_ML = byPlatform({
  macos: true,
  windows: false,
  linux: false,
});

export const HAS_APPLE_INTELLIGENCE = byPlatform({
  macos: true,
  windows: false,
  linux: false,
});

export const UNSUPPORTED_OS_MODEL_REASON = byPlatform({
  macos: "The on-device model needs macOS 26 or later",
  windows: "The on-device model isn't available on this platform",
  linux: "The on-device model isn't available on this platform",
});

export const POWER_SAVING_MODE = byPlatform({
  macos: "Low Power Mode",
  windows: "Battery Saver",
  linux: "power saver",
});

export const LOGIN_ITEM_UNAVAILABLE = byPlatform({
  macos: "Needs macOS 13 or later",
  windows: "Not available on Windows yet",
  linux: "Not available on Linux yet",
});

/**
 * Structural, so a `KeyboardEvent` and a plain object both fit without DOM
 * types.
 */
export interface PrimaryModifierKeys {
  metaKey: boolean;
  ctrlKey: boolean;
}

export function hasPrimaryModifier(event: PrimaryModifierKeys): boolean {
  return byPlatform({
    macos: event.metaKey && !event.ctrlKey,
    windows: event.ctrlKey && !event.metaKey,
    linux: event.ctrlKey && !event.metaKey,
  });
}

/**
 * Redo letter with the primary modifier alone: Ctrl+Y on Windows, Shift+Z only
 * on macOS.
 */
export const REDO_KEY = byPlatform<string | undefined>({
  macos: undefined,
  windows: "y",
  linux: "y",
});

const MAC_KEY_SYMBOLS: Record<string, string> = { Enter: "↵", Backspace: "⌫" };

export function shortcutLabel(
  key: string,
  { shift = false }: { shift?: boolean } = {},
): string {
  const ctrl = ["Ctrl", ...(shift ? ["Shift"] : []), key].join("+");
  return byPlatform({
    macos: `⌘${shift ? "⇧" : ""}${MAC_KEY_SYMBOLS[key] ?? key}`,
    windows: ctrl,
    linux: ctrl,
  });
}
