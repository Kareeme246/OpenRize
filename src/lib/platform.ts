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

/** The OS's bin for deleted files, which removals move things into. */
export const TRASH = byPlatform({
  macos: "Trash",
  windows: "Recycle Bin",
  linux: "Trash",
});

/**
 * Whether the OS draws its own window caption (title and window buttons)
 * above the webview. macOS overlays its title bar on the webview instead
 * (`titleBarStyle: "Overlay"` in tauri.conf.json) and Windows has its caption
 * removed (src-tauri/src/caption.rs), so the app's own bar is the title bar on
 * both.
 */
export const HAS_NATIVE_CAPTION = byPlatform({
  macos: false,
  windows: false,
  linux: true,
});

/**
 * Whether the app draws the minimize, maximize and close buttons itself, at
 * the right end of its top bar. macOS draws its traffic lights over the
 * webview, so only a platform whose caption was removed needs them.
 */
export const DRAWS_CAPTION_BUTTONS = byPlatform({
  macos: false,
  windows: true,
  linux: false,
});

/**
 * Whether a right click on the top bar opens the system window menu (Restore,
 * Move, Size, Minimize, Maximize, Close), as it does on a native caption. Only
 * where the app's bar replaces a caption the OS removed.
 */
export const BAR_OPENS_WINDOW_MENU = byPlatform({
  macos: false,
  windows: true,
  linux: false,
});

/**
 * Whether the window's own theme is kept in step with the app's. Besides a
 * caption it decides the scheme of what the OS draws for the window (native
 * menus, the webview's context menu), so Windows keeps it even without a
 * caption. macOS has none of that to follow.
 */
export const SYNCS_WINDOW_THEME = byPlatform({
  macos: false,
  windows: true,
  linux: true,
});

/**
 * Left padding of the app's top bar. On macOS it clears the traffic lights,
 * which the overlaid title bar draws over the webview's left edge.
 */
export const TOP_BAR_PADDING_LEFT = byPlatform({
  macos: "pl-[84px]",
  windows: "pl-3",
  linux: "pl-3",
});

/**
 * Right padding of the app's top bar. Windows' caption buttons run to the
 * window's edge, so the bar leaves no gap there.
 */
export const TOP_BAR_PADDING_RIGHT = byPlatform({
  macos: "pr-3",
  windows: "pr-0",
  linux: "pr-3",
});

/**
 * Permissions the OS asks the user for (Accessibility, Automation), with the
 * first-run screen that offers them. Only macOS has any.
 */
export const HAS_PERMISSIONS = byPlatform({
  macos: true,
  windows: false,
  linux: false,
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

/** Apple Intelligence, the on-device model that the AI engine can use. */
export const HAS_APPLE_INTELLIGENCE = byPlatform({
  macos: true,
  windows: false,
  linux: false,
});

/** Why the on-device model is unavailable on an OS that cannot run it at all. */
export const UNSUPPORTED_OS_MODEL_REASON = byPlatform({
  macos: "The on-device model needs macOS 26 or later",
  windows: "The on-device model isn't available on this platform",
  linux: "The on-device model isn't available on this platform",
});

/** The OS setting that throttles background work to save battery. */
export const POWER_SAVING_MODE = byPlatform({
  macos: "Low Power Mode",
  windows: "Battery Saver",
  linux: "power saver",
});

/** Why launch at login is unavailable, for the OSes that do not offer it yet. */
export const LOGIN_ITEM_UNAVAILABLE = byPlatform({
  macos: "Needs macOS 13 or later",
  windows: "Not available on Windows yet",
  linux: "Not available on Linux yet",
});

/**
 * The modifier keys a shortcut reads. Structural, so a `KeyboardEvent` and a
 * plain object both fit, and this module needs no DOM types to be imported.
 */
export interface PrimaryModifierKeys {
  metaKey: boolean;
  ctrlKey: boolean;
}

/** The modifier app shortcuts use (and not the other one). */
export function hasPrimaryModifier(event: PrimaryModifierKeys): boolean {
  return byPlatform({
    macos: event.metaKey && !event.ctrlKey,
    windows: event.ctrlKey && !event.metaKey,
    linux: event.ctrlKey && !event.metaKey,
  });
}

/**
 * The letter that redoes with the primary modifier alone (no Shift). Windows
 * editors redo with Ctrl+Y; macOS redoes with Shift+Z only.
 */
export const REDO_KEY = byPlatform<string | undefined>({
  macos: undefined,
  windows: "y",
  linux: "y",
});

/** Keys macOS writes as symbols. Other platforms spell them out. */
const MAC_KEY_SYMBOLS: Record<string, string> = { Enter: "↵", Backspace: "⌫" };

/**
 * A shortcut as the platform writes it: "⌘⇧↵" on macOS, "Ctrl+Shift+Enter"
 * elsewhere.
 */
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
