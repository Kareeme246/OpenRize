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
/** What the AI suggests for each entry (Rize's "Suggestion level"). */
export type AiSuggest = "category" | "categoryProject";

export interface Settings {
  theme: Theme;
  accent: Accent;
  closeBehavior: CloseBehavior;
  trayEnabled: boolean;
  /** Activity history older than this many days. 0 = keep forever. */
  retentionDays: number;
  aiSuggest: AiSuggest;
  /** Approve entries whose every suggested field clears the threshold. */
  autoAccept: boolean;
  autoAcceptPercent: number;
  /** Appended to the on-device model's instructions. */
  aiCustomPrompt: string;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  accent: "green",
  closeBehavior: "hide",
  trayEnabled: true,
  retentionDays: 0,
  aiSuggest: "categoryProject",
  autoAccept: true,
  autoAcceptPercent: 95,
  aiCustomPrompt: "",
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
    base: "#5bcf8f",
    dim: "#48ab74",
    soft: "rgba(91, 207, 143, 0.15)",
    rgb: "91, 207, 143",
    onAccent: "#10261a",
  },
  blue: {
    label: "Royal blue",
    base: "#6086e3",
    dim: "#4a6ec7",
    soft: "rgba(96, 134, 227, 0.15)",
    rgb: "96, 134, 227",
    onAccent: "#ffffff",
  },
  purple: {
    label: "Purple",
    base: "#b07cf0",
    dim: "#945edd",
    soft: "rgba(176, 124, 240, 0.15)",
    rgb: "176, 124, 240",
    onAccent: "#ffffff",
  },
  orange: {
    label: "Orange",
    base: "#e5995c",
    dim: "#c98047",
    soft: "rgba(229, 153, 92, 0.15)",
    rgb: "229, 153, 92",
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
