/**
 * User preferences, mirroring `Settings` in src-tauri/src/settings.rs. The
 * accent palette here is the one place accent colours are defined.
 */

import { getCurrentWindow } from "@tauri-apps/api/window";
import { SYNCS_WINDOW_THEME } from "./platform";

export type Theme = "system" | "light" | "dark";
export type Accent = "green" | "blue" | "purple" | "orange";
export type Shape = "rounded" | "sharper";
export type SizeMode = "compact" | "normal" | "relaxed" | "veryRelaxed";
export type CloseBehavior = "quit" | "hide";
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
 * A day's tracking window in minutes after midnight, or null; `end` below
 * `start` runs past midnight.
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

export interface Weekdays {
  mon: boolean;
  tue: boolean;
  wed: boolean;
  thu: boolean;
  fri: boolean;
  sat: boolean;
  sun: boolean;
}

export interface ScheduledBreak {
  id: string;
  label: string;
  /** `HH:MM`, local time. */
  at: string;
  minutes: number;
  days: Weekdays;
  enabled: boolean;
}

/** Interval reminders have no days: tracking hours decide when they fire. */
export interface BreakSettings {
  enabled: boolean;
  workMinutes: number;
  breakMinutes: number;
  /** What the reminder's snooze button snoozes for; 5, 10 or 15. */
  snoozeMinutes: number;
  message: string;
  pauseStopwatches: boolean;
  chime: boolean;
  schedules: ScheduledBreak[];
}

export const WEEKDAY_KEYS = [
  "mon",
  "tue",
  "wed",
  "thu",
  "fri",
  "sat",
  "sun",
] as const;

export const WEEKDAYS_MON_FRI: Weekdays = {
  mon: true,
  tue: true,
  wed: true,
  thu: true,
  fri: true,
  sat: false,
  sun: false,
};

export const MAX_BREAK_MESSAGE_CHARS = 120;
export const MAX_SCHEDULED_BREAKS = 12;

export const DEFAULT_BREAK_SETTINGS: BreakSettings = {
  enabled: true,
  workMinutes: 50,
  breakMinutes: 5,
  snoozeMinutes: 5,
  message: "",
  pauseStopwatches: false,
  chime: false,
  schedules: [],
};

export type NotificationPlacement =
  | "topLeft"
  | "topRight"
  | "topCenter"
  | "bottomLeft"
  | "bottomRight";

export interface StopwatchReminder {
  enabled: boolean;
  /** Current run length; accumulated time never counts. */
  afterMinutes: number;
}

export interface Settings {
  theme: Theme;
  accent: Accent;
  shape: Shape;
  sizeMode: SizeMode;
  closeBehavior: CloseBehavior;
  trayEnabled: boolean;
  /** Activity history older than this many days. 0 = keep forever. */
  retentionDays: number;
  aiSuggest: AiSuggest;
  autoAccept: boolean;
  autoAcceptPercent: number;
  aiCustomPrompt: string;
  /** Expected work hours per week; a day's target is a fifth of it. */
  weeklyTargetHours: number;
  trackingHours: TrackingHours;
  breaks: BreakSettings;
  stopwatchReminder: StopwatchReminder;
  notificationPlacement: NotificationPlacement;
}

export const DEFAULT_SETTINGS: Settings = {
  theme: "dark",
  accent: "green",
  shape: "rounded",
  sizeMode: "normal",
  closeBehavior: "hide",
  trayEnabled: true,
  retentionDays: 0,
  aiSuggest: "categoryProject",
  autoAccept: true,
  autoAcceptPercent: 95,
  aiCustomPrompt: "",
  weeklyTargetHours: 40,
  trackingHours: DEFAULT_TRACKING_HOURS,
  breaks: DEFAULT_BREAK_SETTINGS,
  stopwatchReminder: { enabled: true, afterMinutes: 180 },
  notificationPlacement: "bottomRight",
};

export function dailyTargetMs(settings: Settings): number {
  return (settings.weeklyTargetHours / 5) * 3_600_000;
}

export function weeklyTargetMs(settings: Settings): number {
  return settings.weeklyTargetHours * 3_600_000;
}

/**
 * Expected work time for a day, week, or month (the weekly target prorated by
 * days).
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

export function formatTargetHours(ms: number): string {
  const hours = ms / 3_600_000;
  return `${hours >= 10 ? Math.round(hours) : Math.round(hours * 10) / 10}h`;
}

export interface AccentPalette {
  label: string;
  base: string;
  dim: string;
  soft: string;
  /** `r, g, b` - fed to rgba() for glows that need an alpha channel. */
  rgb: string;
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

export type LoginItemState =
  | "enabled"
  | "disabled"
  | "requiresApproval"
  | "unsupported";

export interface StoragePaths {
  configFile: string;
  dataDir: string;
  databaseFile: string;
}

/**
 * Mirrors theme and shape for index.html's pre-paint script; keep key names in
 * sync with `appearance_init_script` in src-tauri/src/settings.rs.
 */
const THEME_HINT_KEY = "openrize.theme";
const SHAPE_HINT_KEY = "openrize.shape";

export function resolveTheme(theme: Theme): "light" | "dark" {
  if (theme !== "system") return theme;
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

/** The preference last sent to the window, so a repaint is not an IPC. */
let windowTheme: Theme | null = null;

/**
 * Makes OS-drawn window chrome follow the app theme; main window only,
 * cosmetic, so a failure is dropped and retried.
 */
function syncWindowTheme(theme: Theme): void {
  if (!SYNCS_WINDOW_THEME || windowTheme === theme) return;
  const current = getCurrentWindow();
  if (current.label !== "main") return;
  windowTheme = theme;
  current
    .setTheme(theme === "system" ? null : theme)
    .then(() => {
      // The page's own idea of "system" was borrowed from the window while it
      // was forced to the other scheme; look again now it is released.
      if (theme === "system" && windowTheme === "system") {
        const resolved = resolveTheme("system");
        document.documentElement.dataset.theme = resolved;
        document.documentElement.style.colorScheme = resolved;
      }
    })
    .catch(() => {
      windowTheme = null;
    });
}

/**
 * Theme is an attribute so app.css variable overrides apply; accent is inline
 * on `:root`, beating the `@theme` defaults.
 */
export function applyAppearance(settings: Settings): void {
  const root = document.documentElement;
  const theme = resolveTheme(settings.theme);
  syncWindowTheme(settings.theme);
  try {
    localStorage.setItem(THEME_HINT_KEY, settings.theme);
    localStorage.setItem(SHAPE_HINT_KEY, settings.shape);
  } catch {
    // Storage can be unavailable; the hint only avoids a startup flash.
  }
  root.dataset.theme = theme;
  // Renders native widgets for the active scheme.
  root.style.colorScheme = theme;
  root.dataset.shape = settings.shape;
  root.dataset.sizeMode = settings.sizeMode;

  const accent = ACCENTS[settings.accent];
  root.style.setProperty("--accent", accent.base);
  root.style.setProperty("--accent-dim", accent.dim);
  root.style.setProperty("--accent-soft", accent.soft);
  root.style.setProperty("--accent-rgb", accent.rgb);
  root.style.setProperty("--accent-fg", accent.onAccent);
}
