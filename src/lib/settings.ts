/**
 * User preferences, mirroring `Settings` in src-tauri/src/settings.rs.
 *
 * The accent palette is the ONE place accent colours are defined. `applyAccent`
 * writes them onto `:root` as CSS variables, and every Tailwind utility
 * (`bg-accent`, `text-accent`, `border-accent/30`, ...) reads those variables -
 * so changing a hex here repaints the entire app, no component edits.
 */

export type Theme = "system" | "light" | "dark";
export type Accent = "green" | "blue" | "purple" | "orange";
export type CloseBehavior = "quit" | "hide";

export interface Settings {
  theme: Theme;
  accent: Accent;
  closeBehavior: CloseBehavior;
  trayEnabled: boolean;
  /** Activity history older than this many days. 0 = keep forever. */
  retentionDays: number;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  accent: "green",
  closeBehavior: "hide",
  trayEnabled: true,
  retentionDays: 0,
};

export interface AccentPalette {
  label: string;
  /** Solid accent used for highlights, fills, and the active glyph. */
  base: string;
  /** Darker stop for the gradient buttons. */
  dim: string;
  /** Translucent accent for selected rows and soft fills. */
  soft: string;
  /** `r, g, b` - fed to rgba() for glows that need an alpha channel. */
  rgb: string;
  /** Readable text on top of `base`. */
  onAccent: string;
}

export const ACCENTS: Record<Accent, AccentPalette> = {
  green: {
    label: "Green",
    base: "#2aea83",
    dim: "#1fb968",
    soft: "rgba(42, 234, 131, 0.15)",
    rgb: "42, 234, 131",
    onAccent: "#04160c",
  },
  blue: {
    label: "Royal blue",
    base: "#3b6bf0",
    dim: "#2a4fd0",
    soft: "rgba(59, 107, 240, 0.15)",
    rgb: "59, 107, 240",
    onAccent: "#ffffff",
  },
  purple: {
    label: "Purple",
    base: "#a259ff",
    dim: "#7f36e0",
    soft: "rgba(162, 89, 255, 0.15)",
    rgb: "162, 89, 255",
    onAccent: "#ffffff",
  },
  orange: {
    label: "Orange",
    base: "#ff8a2b",
    dim: "#d96a12",
    soft: "rgba(255, 138, 43, 0.15)",
    rgb: "255, 138, 43",
    onAccent: "#241200",
  },
};

export const ACCENT_ORDER = Object.keys(ACCENTS) as Accent[];

export interface StoragePaths {
  configFile: string;
  dataDir: string;
  /** Trackers and activity share this one SQLite file (decision A8). */
  databaseFile: string;
}

/** Which concrete palette a preference means right now. */
export function resolveTheme(theme: Theme): "light" | "dark" {
  if (theme !== "system") return theme;
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

/**
 * Paints the current preference onto the document. Theme is an attribute
 * (`[data-theme]`) so the CSS variable overrides in app.css apply; accent is
 * set inline on `:root`, which wins over the `@theme` defaults.
 */
export function applyAppearance(settings: Settings): void {
  const root = document.documentElement;
  const theme = resolveTheme(settings.theme);
  root.dataset.theme = theme;
  // Tells the browser to render native widgets (scrollbars, form controls)
  // for the active scheme.
  root.style.colorScheme = theme;

  const accent = ACCENTS[settings.accent];
  root.style.setProperty("--accent", accent.base);
  root.style.setProperty("--accent-dim", accent.dim);
  root.style.setProperty("--accent-soft", accent.soft);
  root.style.setProperty("--accent-rgb", accent.rgb);
  root.style.setProperty("--accent-fg", accent.onAccent);
}
