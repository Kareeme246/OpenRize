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

export interface DaySchedule {
  enabled: boolean;
  start: string;
  end: string;
}

export interface TrackingHours {
  enabled: boolean;
  perDay: boolean;
  defaultStart: string;
  defaultEnd: string;
  monday: DaySchedule;
  tuesday: DaySchedule;
  wednesday: DaySchedule;
  thursday: DaySchedule;
  friday: DaySchedule;
  saturday: DaySchedule;
  sunday: DaySchedule;
}

export const DEFAULT_DAY_SCHEDULE: DaySchedule = {
  enabled: true,
  start: "07:00",
  end: "19:00",
};

export const DEFAULT_TRACKING_HOURS: TrackingHours = {
  enabled: true,
  perDay: false,
  defaultStart: "07:00",
  defaultEnd: "19:00",
  monday: { ...DEFAULT_DAY_SCHEDULE },
  tuesday: { ...DEFAULT_DAY_SCHEDULE },
  wednesday: { ...DEFAULT_DAY_SCHEDULE },
  thursday: { ...DEFAULT_DAY_SCHEDULE },
  friday: { ...DEFAULT_DAY_SCHEDULE },
  saturday: { ...DEFAULT_DAY_SCHEDULE },
  sunday: { ...DEFAULT_DAY_SCHEDULE },
};

const DAY_KEYS = [
  "sunday",
  "monday",
  "tuesday",
  "wednesday",
  "thursday",
  "friday",
  "saturday",
] as const;

/** `"07:30"` -> 450, or undefined when malformed. */
function minutesOf(clock: string): number | undefined {
  const [hours, minutes] = clock.split(":").map(Number);
  if (hours === undefined || minutes === undefined) return undefined;
  if (Number.isNaN(hours) || Number.isNaN(minutes)) return undefined;
  return hours * 60 + minutes;
}

/**
 * A day's tracking window in minutes after midnight, or null when that day
 * does not track. `end` below `start` means the window runs past midnight.
 */
function dayWindow(
  th: TrackingHours,
  date: Date,
): { start: number; end: number } | null {
  const dayKey = DAY_KEYS[date.getDay()];
  if (!dayKey) return null;
  const [start, end, enabled] = th.perDay
    ? [th[dayKey].start, th[dayKey].end, th[dayKey].enabled]
    : [th.defaultStart, th.defaultEnd, true];
  if (!enabled || start === end) return null;
  const startMins = minutesOf(start);
  const endMins = minutesOf(end);
  if (startMins === undefined || endMins === undefined) return null;
  return { start: startMins, end: endMins };
}

/** Whether the given date is inside the configured tracking hours window. */
export function isInsideTrackingHours(
  th: TrackingHours,
  date: Date = new Date(),
): boolean {
  if (!th.enabled) return true;
  const window = dayWindow(th, date);
  if (!window) return false;
  const curMins = date.getHours() * 60 + date.getMinutes();
  if (window.start < window.end) {
    return curMins >= window.start && curMins < window.end;
  }
  return curMins >= window.start || curMins < window.end;
}

/** When the schedule next starts tracking, within a week; null if never. */
export function nextTrackingStart(
  th: TrackingHours,
  now: Date = new Date(),
): Date | null {
  if (!th.enabled) return null;
  for (let offset = 0; offset <= 7; offset++) {
    const day = new Date(now);
    day.setDate(day.getDate() + offset);
    const window = dayWindow(th, day);
    if (!window) continue;
    day.setHours(0, window.start, 0, 0);
    if (day.getTime() > now.getTime()) return day;
  }
  return null;
}

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
  /** Expected work hours per week; a day's target is a fifth of it. */
  weeklyTargetHours: number;
  trackingHours: TrackingHours;
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
  weeklyTargetHours: 40,
  trackingHours: DEFAULT_TRACKING_HOURS,
};

/** A working day's share of the weekly target, in milliseconds. */
export function dailyTargetMs(settings: Settings): number {
  return (settings.weeklyTargetHours / 5) * 3_600_000;
}

export function weeklyTargetMs(settings: Settings): number {
  return settings.weeklyTargetHours * 3_600_000;
}

/**
 * Expected work time for a Calendar-style range: a day's or a week's share of
 * the weekly target, or for a month the weekly target prorated by its days.
 */
export function targetMsFor(
  settings: Settings,
  scale: "day" | "week" | "month",
  days: number,
): number {
  if (scale === "day") return dailyTargetMs(settings);
  if (scale === "week") return weeklyTargetMs(settings);
  return weeklyTargetMs(settings) * (days / 7);
}

/** A target in hours: "7.5h" for a short one, whole hours ("171h") above 10. */
export function formatTargetHours(ms: number): string {
  const hours = ms / 3_600_000;
  return `${hours >= 10 ? Math.round(hours) : Math.round(hours * 10) / 10}h`;
}

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

/** Mirror of `LoginItemState` in src-tauri/src/login_item.rs. */
export type LoginItemState =
  | "enabled"
  | "disabled"
  | "requiresApproval"
  | "unsupported";

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
