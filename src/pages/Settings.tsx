import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import {
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import {
  AiEffectiveness,
  PersonalModels,
  ThresholdSlider,
} from "../components/AiLearning";
import { BatteryEnergyMonitor } from "../components/BatteryEnergyMonitor";
import { ConfirmDialog } from "../components/ConfirmDialog";
import { Picker } from "../components/Picker";
import {
  SegmentedControl,
  type SegmentedOption,
  Toggle,
} from "../components/SegmentedControl";
import { type Status, StatusBadge } from "../components/StatusBadge";
import { useAiMetrics } from "../hooks/useAiMetrics";
import { llmUnavailableReason, useAiStatus } from "../hooks/useAiStatus";
import { useEnergy } from "../hooks/useEnergy";
import { useSettings } from "../hooks/useSettings";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { formatRelative, formatShortDate } from "../lib/format";
import {
  ACCENT_ORDER,
  ACCENTS,
  type Accent,
  type AiSuggest,
  type CloseBehavior,
  type DaySchedule,
  isInsideTrackingHours,
  type LoginItemState,
  type Settings as SettingsType,
  type SizeMode,
  type Theme,
  type TrackingHours,
} from "../lib/settings";
import type { AiStatus, ReleaseNotes, Route, UpdateStatus } from "../lib/types";

const THEME_OPTIONS: SegmentedOption<Theme>[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const SIZE_OPTIONS: SegmentedOption<SizeMode>[] = [
  { value: "compact", label: "Compact" },
  { value: "normal", label: "Normal" },
  { value: "relaxed", label: "Relaxed" },
  { value: "veryRelaxed", label: "Very Relaxed" },
];

const ACCENT_OPTIONS: SegmentedOption<Accent>[] = ACCENT_ORDER.map(
  (accent) => ({
    value: accent,
    label: ACCENTS[accent].label,
    swatch: ACCENTS[accent].base,
  }),
);

const CLOSE_OPTIONS: SegmentedOption<CloseBehavior>[] = [
  { value: "hide", label: "Hide to menu bar" },
  { value: "quit", label: "Quit" },
];

const SUGGEST_OPTIONS: SegmentedOption<AiSuggest>[] = [
  { value: "category", label: "Category" },
  { value: "categoryProject", label: "Category + Project" },
];

const METRIC_RANGES: SegmentedOption<number>[] = [
  { value: 7, label: "7d" },
  { value: 30, label: "30d" },
  { value: 90, label: "90d" },
];

type SettingsSection =
  | "overview"
  | "appearance"
  | "general"
  | "tracking"
  | "suggestions"
  | "data"
  | "notifications"
  | "advanced";

interface SettingsNavItem {
  id: SettingsSection;
  label: string;
  hint?: string;
}

interface SettingsNavGroup {
  label: string;
  items: SettingsNavItem[];
}

const SETTINGS_NAV_GROUPS: SettingsNavGroup[] = [
  {
    label: "Preferences",
    items: [
      { id: "appearance", label: "Appearance" },
      { id: "general", label: "General" },
      { id: "tracking", label: "Tracking & work" },
      { id: "suggestions", label: "Suggestions" },
      { id: "data", label: "Data & storage" },
      { id: "notifications", label: "Notifications" },
    ],
  },
  {
    label: "Advanced",
    items: [
      {
        id: "advanced",
        label: "Advanced settings",
        hint: "AI effectiveness, personal models, Battery & Energy",
      },
    ],
  },
];

const OVERVIEW_LINKS: {
  section: Exclude<SettingsSection, "overview" | "notifications">;
  title: string;
  description: string;
}[] = [
  {
    section: "appearance",
    title: "Appearance",
    description: "Theme and accent colour",
  },
  {
    section: "tracking",
    title: "Tracking & work",
    description: "Schedule and weekly target",
  },
  {
    section: "suggestions",
    title: "Suggestions",
    description: "Category and project behavior",
  },
  {
    section: "advanced",
    title: "Advanced",
    description: "AI effectiveness and Battery & Energy",
  },
];

/** One line on which tiers are running and why. */
function engineSummary(status: AiStatus | null): string {
  if (status === null || status.engine === "starting") {
    return "Starting the on-device models…";
  }
  if (status.engine === "full") {
    return "Rules, your personal model, and Apple's on-device model";
  }
  if (status.engine === "fallback") {
    return `Using rules and your personal model only. ${llmUnavailableReason(status) ?? ""}.`;
  }
  return "Using your rules only: on-device ML isn't available in this build";
}

/** 10–60 hours a week in steps of 5, plus a saved value off that grid. */
function targetOptions(current: number): { value: number; label: string }[] {
  const values = new Set([current]);
  for (let hours = 10; hours <= 60; hours += 5) values.add(hours);
  return [...values]
    .sort((a, b) => a - b)
    .map((hours) => ({ value: hours, label: `${hours}h a week` }));
}

const RETENTION_OPTIONS = [
  { value: 0, label: "Forever" },
  { value: 7, label: "7 days" },
  { value: 30, label: "30 days" },
] satisfies { value: number; label: string }[];

/** Title + description with an inline control on the right. */
function SettingRow({
  title,
  description,
  status,
  children,
}: {
  title: string;
  description: string;
  status?: Status;
  children?: ReactNode;
}) {
  return (
    <div className="setting-row flex items-center justify-between gap-4 border-b border-line px-4 py-3 last:border-b-0">
      <div className="min-w-0">
        <div className="text-[13px] font-medium text-fg">{title}</div>
        <div className="text-[12px] leading-relaxed text-fg-muted">
          {description}
        </div>
      </div>
      {children ?? (status !== undefined && <StatusBadge status={status} />)}
    </div>
  );
}

/** Title + description with a full-width control underneath. */
function SettingBlock({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="setting-block border-b border-line px-4 py-3 last:border-b-0">
      <div className="text-[13px] font-medium text-fg">{title}</div>
      <div className="text-[12px] leading-relaxed text-fg-muted">
        {description}
      </div>
      <div className="mt-3">{children}</div>
    </div>
  );
}

function SettingGroup({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="px-1 font-mono text-[11px] font-semibold uppercase tracking-wider text-fg-muted">
        {title}
      </h2>
      <div className="settings-group-card rounded-xl border border-settings-card-border bg-settings-card">
        {children}
      </div>
    </section>
  );
}

function SettingsSectionHeading({
  title,
  description,
}: {
  title: string;
  description: string;
}) {
  return (
    <header className="settings-section-heading">
      <h2 className="font-sans text-[20px] font-semibold tracking-tight text-fg-strong">
        {title}
      </h2>
      <p className="mt-1 max-w-2xl text-[12px] leading-relaxed text-fg-muted">
        {description}
      </p>
    </header>
  );
}

const LOGIN_ITEMS_SETTINGS =
  "x-apple.systempreferences:com.apple.LoginItems-Settings.extension";

/**
 * Mirrors macOS's login item rather than a stored preference: it is re-read
 * whenever the window regains focus, because the user can switch it off in
 * System Settings behind the app's back.
 */
function LaunchAtLoginRow({
  onError,
}: {
  onError: (message: string | null) => void;
}) {
  const [state, setState] = useState<LoginItemState | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const read = (): void => {
      api
        .launchAtLogin()
        .then(setState)
        .catch((cause: unknown) => onError(describeError(cause)));
    };
    read();
    window.addEventListener("focus", read);
    return () => window.removeEventListener("focus", read);
  }, [onError]);

  const change = (enabled: boolean): void => {
    setBusy(true);
    onError(null);
    api
      .setLaunchAtLogin(enabled)
      .then(setState)
      .catch((cause: unknown) => onError(describeError(cause)))
      .finally(() => setBusy(false));
  };

  const pending = state === "requiresApproval";
  return (
    <SettingRow
      title="Launch at login"
      description={
        state === "unsupported"
          ? "Needs macOS 13 or later"
          : pending
            ? "Allow OpenRize under Login Items in System Settings to finish"
            : "Start OpenRize in the background when you log in"
      }
    >
      <div className="flex shrink-0 items-center gap-3">
        {pending && (
          <button
            type="button"
            onClick={() => {
              openUrl(LOGIN_ITEMS_SETTINGS).catch((cause: unknown) =>
                onError(describeError(cause)),
              );
            }}
            className="rounded-md border border-line px-2.5 py-1 text-[11.5px] text-fg-muted transition-colors hover:bg-surface hover:text-fg"
          >
            Open Login Items
          </button>
        )}
        <Toggle
          checked={state === "enabled" || pending}
          label="Launch at login"
          disabled={state === null || state === "unsupported" || busy}
          onChange={change}
        />
      </div>
    </SettingRow>
  );
}

function PathLine({ label, path }: { label: string; path: string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="font-mono text-[10px] uppercase tracking-wider text-fg-muted">
        {label}
      </span>
      <code
        title={path}
        className="truncate rounded-lg border border-line-soft bg-inset-soft px-2.5 py-1.5 font-mono text-[11.5px] text-fg-muted"
      >
        {path}
      </code>
    </div>
  );
}

function FileLine({ label, path }: { label: string; path: string }) {
  return (
    <div className="flex items-center justify-between gap-3 text-[11.5px]">
      <span className="shrink-0 text-fg-muted">{label}</span>
      <span
        title={path}
        className="min-w-0 truncate font-mono text-[11px] text-fg-muted"
      >
        {path}
      </span>
    </div>
  );
}

/** A themed dropdown, styled to match the segmented controls beside it. */
function Select({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: number;
  options: { value: number; label: string }[];
  onChange: (value: number) => void;
}) {
  return (
    <Picker<number>
      ariaLabel={label}
      value={value}
      options={options}
      onChange={onChange}
      variant="compact"
    />
  );
}

/** Saved on blur, so typing doesn't rewrite settings.json per keystroke. */
function CustomInstructions({
  value,
  onSave,
}: {
  value: string;
  onSave: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const [focused, setFocused] = useState(false);
  const shown = focused ? draft : value;
  return (
    <SettingBlock
      title="Custom instructions"
      description="Added to the on-device model's prompt, e.g. “Anything in the OpenRize repo is Coding”"
    >
      <textarea
        aria-label="Custom instructions"
        value={shown}
        maxLength={1000}
        rows={3}
        onFocus={() => {
          setDraft(value);
          setFocused(true);
        }}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => {
          setFocused(false);
          if (draft !== value) onSave(draft);
        }}
        className="mt-2 w-full resize-y rounded-lg border border-line bg-surface px-2.5 py-2 text-[12px] text-fg outline-hidden placeholder:text-fg-muted focus:border-accent"
        placeholder="No custom instructions"
      />
    </SettingBlock>
  );
}

type DayKey =
  | "monday"
  | "tuesday"
  | "wednesday"
  | "thursday"
  | "friday"
  | "saturday"
  | "sunday";

const DAYS_OF_WEEK: { key: DayKey; label: string }[] = [
  { key: "monday", label: "Monday" },
  { key: "tuesday", label: "Tuesday" },
  { key: "wednesday", label: "Wednesday" },
  { key: "thursday", label: "Thursday" },
  { key: "friday", label: "Friday" },
  { key: "saturday", label: "Saturday" },
  { key: "sunday", label: "Sunday" },
];

function TrackingHoursSetting({
  settings,
  update,
}: {
  settings: SettingsType;
  update: (patch: Partial<SettingsType>) => void;
}) {
  const th = settings.trackingHours;
  const inHours = isInsideTrackingHours(th);

  const updateHours = (patch: Partial<TrackingHours>): void => {
    update({
      trackingHours: {
        ...th,
        ...patch,
      },
    });
  };

  const updateDay = (dayKey: DayKey, patch: Partial<DaySchedule>): void => {
    updateHours({
      [dayKey]: {
        ...th[dayKey],
        ...patch,
      },
    });
  };

  return (
    <SettingGroup title="Tracking hours">
      <SettingRow
        title="Schedule tracking hours"
        description="Only capture activity during scheduled hours; outside them, tracking stays paused unless manually started"
      >
        <Toggle
          checked={th.enabled}
          label="Schedule tracking hours"
          onChange={(enabled) => updateHours({ enabled })}
        />
      </SettingRow>

      {th.enabled &&
        (!th.perDay ? (
          <>
            <SettingRow
              title="Daily window"
              description="7:00 AM to 7:00 PM by default, applying to every day"
            >
              <fieldset className="settings-time-range m-0 flex min-w-0 items-end gap-2.5 border-0 p-0">
                <legend className="sr-only">Daily tracking hours</legend>
                <label className="flex flex-col gap-1">
                  <span className="font-mono text-[10px] uppercase tracking-wider text-fg-muted">
                    Start
                  </span>
                  <input
                    type="time"
                    value={th.defaultStart}
                    onChange={(e) =>
                      updateHours({ defaultStart: e.target.value })
                    }
                    className="h-7 rounded-md border border-line bg-surface px-2 py-1 text-[12px] text-fg outline-hidden focus:border-accent"
                  />
                </label>
                <span className="pb-1 text-[12px] text-fg-muted">to</span>
                <label className="flex flex-col gap-1">
                  <span className="font-mono text-[10px] uppercase tracking-wider text-fg-muted">
                    End
                  </span>
                  <input
                    type="time"
                    value={th.defaultEnd}
                    onChange={(e) =>
                      updateHours({ defaultEnd: e.target.value })
                    }
                    className="h-7 rounded-md border border-line bg-surface px-2 py-1 text-[12px] text-fg outline-hidden focus:border-accent"
                  />
                </label>
              </fieldset>
            </SettingRow>
            <div className="settings-inline-note flex items-center justify-between border-b border-line px-4 py-2.5 last:border-b-0">
              <span className="font-mono text-[10.5px] text-fg-muted">
                {inHours
                  ? "Currently within tracking window"
                  : "Currently outside tracking window"}
              </span>
              <button
                type="button"
                onClick={() => updateHours({ perDay: true })}
                className="flex items-center gap-1 text-[12px] font-medium text-accent hover:underline cursor-pointer"
              >
                Customize per day
                <svg
                  viewBox="0 0 24 24"
                  className="size-3.5 fill-none stroke-current stroke-2"
                  aria-hidden="true"
                >
                  <polyline points="6 9 12 15 18 9" />
                </svg>
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="settings-inline-note flex items-center justify-between border-b border-line px-4 py-2.5">
              <span className="text-[12px] text-fg-muted">
                Set hours for each day of the week. Unchecked days will not
                track.
              </span>
              <button
                type="button"
                onClick={() => updateHours({ perDay: false })}
                className="flex items-center gap-1 text-[12px] font-medium text-accent hover:underline cursor-pointer"
              >
                Collapse to single window
                <svg
                  viewBox="0 0 24 24"
                  className="size-3.5 fill-none stroke-current stroke-2"
                  aria-hidden="true"
                >
                  <polyline points="18 15 12 9 6 15" />
                </svg>
              </button>
            </div>
            {DAYS_OF_WEEK.map(({ key, label }) => {
              const day = th[key];
              return (
                <div
                  key={key}
                  className="settings-day-row flex items-center justify-between gap-4 border-b border-line px-4 py-2.5 last:border-b-0"
                >
                  <label className="flex items-center gap-2.5 cursor-pointer select-none">
                    <input
                      type="checkbox"
                      checked={day.enabled}
                      onChange={(e) =>
                        updateDay(key, { enabled: e.target.checked })
                      }
                      className="rounded border-line text-accent focus:ring-accent"
                    />
                    <span
                      className={`text-[13px] font-medium ${
                        day.enabled ? "text-fg" : "text-fg-muted"
                      }`}
                    >
                      {label}
                    </span>
                  </label>
                  {day.enabled ? (
                    <div className="flex items-center gap-2">
                      <input
                        type="time"
                        aria-label={`${label} tracking start time`}
                        value={day.start}
                        onChange={(e) =>
                          updateDay(key, { start: e.target.value })
                        }
                        className="h-7 rounded-md border border-line bg-surface px-2 py-1 text-[12px] text-fg outline-hidden focus:border-accent"
                      />
                      <span className="text-[12px] text-fg-muted">to</span>
                      <input
                        type="time"
                        aria-label={`${label} tracking end time`}
                        value={day.end}
                        onChange={(e) =>
                          updateDay(key, { end: e.target.value })
                        }
                        className="h-7 rounded-md border border-line bg-surface px-2 py-1 text-[12px] text-fg outline-hidden focus:border-accent"
                      />
                    </div>
                  ) : (
                    <span className="text-[12px] text-fg-muted italic">
                      No tracking
                    </span>
                  )}
                </div>
              );
            })}
            <div className="settings-inline-note flex items-center justify-between border-b border-line px-4 py-2.5 last:border-b-0">
              <span className="font-mono text-[10.5px] text-fg-muted">
                {inHours
                  ? "Currently within tracking window"
                  : "Currently outside tracking window"}
              </span>
            </div>
          </>
        ))}
    </SettingGroup>
  );
}

/** "Checking…", "Downloading 42%", or when the last check ran. */
function updateSummary(status: UpdateStatus, now: number): string {
  switch (status.phase.kind) {
    case "checking":
      return "Checking for updates…";
    case "downloading": {
      const { downloaded, total } = status.phase;
      return total
        ? `Downloading ${Math.min(100, Math.round((downloaded / total) * 100))}%…`
        : "Downloading…";
    }
    case "installing":
      return "Installing - OpenRize will restart";
    case "idle":
      break;
  }
  if (status.available) {
    return `Version ${status.available.version} is available. You have ${status.currentVersion}.`;
  }
  return status.lastCheckedMs === null
    ? `You have ${status.currentVersion}`
    : `Up to date · Checked ${formatRelative(status.lastCheckedMs, now).toLowerCase()}`;
}

/**
 * git-cliff writes Markdown; shown as plain text, minus the heading and bold
 * markers that would otherwise read as noise.
 */
function plainNotes(notes: string): string {
  return (
    notes
      .split("\n")
      // The "## [0.4.7] - 2026-09-28" title repeats the row's own header.
      .filter((line) => !/^##\s*\[[^\]]*\]/.test(line))
      .map((line) =>
        line
          .replace(/^#+\s*/, "")
          .replace(/\*\*(.+?)\*\*/g, "$1")
          .replace(/^(\s*)[-*]\s+/, "$1• "),
      )
      .join("\n")
      .trim()
  );
}

function ReleaseNotesList({ releases }: { releases: ReleaseNotes[] }) {
  return (
    <div className="max-h-56 overflow-y-auto overscroll-contain rounded-lg border border-line bg-inset-soft">
      {releases.map((release) => (
        <div
          key={release.version}
          className="border-b border-line-soft px-3 py-2.5 last:border-b-0"
        >
          <div className="flex items-baseline justify-between gap-3">
            <span className="font-mono text-[11.5px] font-semibold text-fg">
              {release.version}
            </span>
            {release.publishedAt && (
              <span className="text-[11px] text-fg-muted">
                {formatShortDate(Date.parse(release.publishedAt))}
              </span>
            )}
          </div>
          <pre className="mt-1.5 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-words text-fg-muted">
            {plainNotes(release.notes) || "No release notes."}
          </pre>
        </div>
      ))}
    </div>
  );
}

/**
 * The updater's status and controls. The Rust side checks on its own
 * schedule (see src-tauri/src/updater.rs); this only shows the result.
 */
function UpdatesSetting({ status }: { status: UpdateStatus | null }) {
  const [now, setNow] = useState(() => Date.now());
  const [requestError, setRequestError] = useState<string | null>(null);

  // Keeps "Checked 5 min ago" honest while the page stays open.
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(id);
  }, []);
  useEffect(() => {
    if (status?.lastCheckedMs) setNow(Date.now());
  }, [status?.lastCheckedMs]);

  const check = (): void => {
    setRequestError(null);
    api
      .checkForUpdates()
      .catch((cause: unknown) => setRequestError(describeError(cause)));
  };
  const install = (): void => {
    setRequestError(null);
    api
      .installUpdate()
      .catch((cause: unknown) => setRequestError(describeError(cause)));
  };

  const busy = status !== null && status.phase.kind !== "idle";
  const error = requestError ?? status?.error ?? null;
  const available = status?.available ?? null;

  return (
    <SettingGroup title="Updates">
      <SettingRow
        title="OpenRize updates"
        description={status === null ? "Loading…" : updateSummary(status, now)}
      >
        {available ? (
          <button
            type="button"
            disabled={busy}
            onClick={install}
            className="shrink-0 rounded-lg bg-accent px-3 py-1.5 font-semibold text-[12px] text-accent-fg transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            Install &amp; restart
          </button>
        ) : (
          <button
            type="button"
            disabled={busy || status === null}
            onClick={check}
            className="shrink-0 rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] text-fg-muted hover:bg-surface-strong disabled:cursor-not-allowed disabled:opacity-50"
          >
            Check for updates
          </button>
        )}
      </SettingRow>
      {error && (
        <div className="border-b border-line px-4 py-2 text-[11.5px] text-danger last:border-b-0">
          {error}
        </div>
      )}
      {available && (
        <SettingBlock
          title="What's new"
          description="Installing restarts OpenRize. A running tracking session ends and a new one starts when it reopens."
        >
          <ReleaseNotesList releases={available.releases} />
        </SettingBlock>
      )}
    </SettingGroup>
  );
}

export function Settings({
  route,
  updates,
  revealUpdates,
}: {
  route: Extract<Route, { name: "settings" }>;
  updates: UpdateStatus | null;
  /** Bumped each time the sidebar asks for the Updates section. */
  revealUpdates: number;
}) {
  const { settings, storage, error, update } = useSettings();
  const [openError, setOpenError] = useState<string | null>(null);
  const [loginError, setLoginError] = useState<string | null>(null);

  const openStorage = (): void => {
    if (storage === null) return;
    setOpenError(null);
    openPath(storage.dataDir).catch((cause: unknown) =>
      setOpenError(describeError(cause)),
    );
  };

  const aiStatus = useAiStatus();
  const [metricDays, setMetricDays] = useState(30);
  const { metrics, error: metricsError, adopt } = useAiMetrics(metricDays);
  const [confirmReset, setConfirmReset] = useState(false);
  const [aiError, setAiError] = useState<string | null>(null);
  const saveThreshold = useCallback(
    (autoAcceptPercent: number) => update({ autoAcceptPercent }),
    [update],
  );

  const retrain = (): void => {
    setAiError(null);
    api.aiRetrain().catch((cause: unknown) => setAiError(describeError(cause)));
  };

  const resetLearned = (): void => {
    setConfirmReset(false);
    setAiError(null);
    api
      .aiResetLearned(metricDays)
      .then(adopt)
      .catch((cause: unknown) => setAiError(describeError(cause)));
  };

  const [energyDays, setEnergyDays] = useState(7);
  const {
    summary: energySummary,
    error: energyError,
    loading: energyLoading,
    resetHistory: resetEnergyHistory,
  } = useEnergy(energyDays);

  const [activeSection, setActiveSection] =
    useState<SettingsSection>("overview");
  const contentRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (route.section !== "updates" && revealUpdates === 0) return;
    setActiveSection("overview");
    contentRef.current?.scrollTo({ top: 0 });
  }, [route.section, revealUpdates]);

  const selectSection = (section: SettingsSection): void => {
    setActiveSection(section);
    contentRef.current?.scrollTo({ top: 0 });
  };

  const trayOff = !settings.trayEnabled;
  const sectionError =
    activeSection === "general"
      ? loginError
      : activeSection === "data"
        ? openError
        : activeSection === "suggestions"
          ? metricsError
          : activeSection === "advanced"
            ? (aiError ?? metricsError ?? energyError)
            : null;
  const problem = error ?? sectionError;

  return (
    <main className="settings-page flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-canvas font-sans text-fg">
      <header className="settings-header flex shrink-0 items-center justify-between gap-4 border-b border-line px-5 py-3">
        <div>
          <h1 className="text-[16px] font-semibold text-fg-strong">Settings</h1>
          <p className="mt-0.5 text-[11px] text-fg-muted">
            Preferences save automatically
          </p>
        </div>
        <span className="shrink-0 font-mono text-[10.5px] text-fg-muted">
          ⌘, opens this
        </span>
      </header>

      <div className="settings-layout flex min-h-0 min-w-0 flex-1">
        <nav
          aria-label="Settings sections"
          className="settings-nav w-48 shrink-0 overflow-y-auto border-r border-line bg-rail px-3 py-4"
        >
          <button
            type="button"
            aria-current={activeSection === "overview" ? "page" : undefined}
            onClick={() => selectSection("overview")}
            className={`settings-nav-item ${activeSection === "overview" ? "settings-nav-item-active" : ""}`}
          >
            Overview
          </button>
          {SETTINGS_NAV_GROUPS.map((group) => (
            <div className="settings-nav-group" key={group.label}>
              <h2 className="settings-nav-group-title">{group.label}</h2>
              {group.items.map((item) => (
                <button
                  type="button"
                  key={item.id}
                  aria-current={activeSection === item.id ? "page" : undefined}
                  onClick={() => selectSection(item.id)}
                  className={`settings-nav-item ${activeSection === item.id ? "settings-nav-item-active" : ""}`}
                >
                  <span>{item.label}</span>
                  {item.hint && (
                    <span className="settings-nav-item-hint">{item.hint}</span>
                  )}
                </button>
              ))}
            </div>
          ))}
        </nav>

        <div
          ref={contentRef}
          className="settings-content min-h-0 min-w-0 flex-1 overflow-y-auto overscroll-contain p-5"
        >
          {problem !== null && (
            <p
              role="alert"
              className="mb-4 rounded-[10px] border border-danger/40 bg-danger-soft px-3.5 py-2.5 text-[13px] text-danger"
            >
              {problem}
            </p>
          )}

          {activeSection === "overview" && (
            <section className="flex flex-col gap-4">
              <SettingsSectionHeading
                title="Overview"
                description="Updates and shortcuts to the settings you use most."
              />
              <UpdatesSetting status={updates} />
              <section aria-labelledby="settings-shortcuts-heading">
                <h3
                  id="settings-shortcuts-heading"
                  className="mb-2 px-1 font-mono text-[11px] font-semibold uppercase tracking-wider text-fg-muted"
                >
                  Settings areas
                </h3>
                <div className="settings-shortcut-grid grid grid-cols-2 gap-3">
                  {OVERVIEW_LINKS.map((link) => (
                    <button
                      type="button"
                      key={link.section}
                      onClick={() => selectSection(link.section)}
                      className="settings-shortcut-card group rounded-xl border border-settings-card-border bg-settings-card p-4 text-left transition-colors hover:border-accent/50 hover:bg-surface"
                    >
                      <span className="flex items-center justify-between gap-3">
                        <span className="font-medium text-fg-strong">
                          {link.title}
                        </span>
                        <span
                          aria-hidden="true"
                          className="text-fg-muted transition-transform group-hover:translate-x-0.5"
                        >
                          →
                        </span>
                      </span>
                      <span className="mt-1 block text-[12px] leading-relaxed text-fg-muted">
                        {link.description}
                      </span>
                    </button>
                  ))}
                </div>
              </section>
            </section>
          )}

          {activeSection === "appearance" && (
            <section className="flex flex-col gap-4">
              <SettingsSectionHeading
                title="Appearance"
                description="Set the reading theme and accent used across OpenRize."
              />
              <SettingGroup title="Theme & accent">
                <SettingRow
                  title="Theme"
                  description="Dark, light, or follow macOS"
                >
                  <SegmentedControl
                    name="theme"
                    value={settings.theme}
                    options={THEME_OPTIONS}
                    onChange={(theme) => update({ theme })}
                  />
                </SettingRow>
                <SettingBlock
                  title="Size mode"
                  description="Scale text and icons throughout OpenRize"
                >
                  <SegmentedControl
                    name="size-mode"
                    value={settings.sizeMode}
                    options={SIZE_OPTIONS}
                    largeTarget
                    onChange={(sizeMode) => update({ sizeMode })}
                  />
                </SettingBlock>
                <SettingRow
                  title="Accent colour"
                  description="Highlights, active states, and running timers"
                >
                  <SegmentedControl
                    name="accent"
                    value={settings.accent}
                    options={ACCENT_OPTIONS}
                    onChange={(accent) => update({ accent })}
                  />
                </SettingRow>
              </SettingGroup>
            </section>
          )}

          {activeSection === "general" && (
            <section className="flex flex-col gap-4">
              <SettingsSectionHeading
                title="General"
                description="Control how OpenRize starts and behaves when its window closes."
              />
              <SettingGroup title="App behavior">
                <LaunchAtLoginRow onError={setLoginError} />
                <SettingRow
                  title="Menu bar icon"
                  description="Keep a tray icon with quick controls"
                >
                  <Toggle
                    checked={settings.trayEnabled}
                    label="Menu bar icon"
                    onChange={(trayEnabled) => update({ trayEnabled })}
                  />
                </SettingRow>
                <SettingRow
                  title="When I close the window"
                  description={
                    trayOff
                      ? "Menu bar icon is off, so closing always quits"
                      : "What the close button does while OpenRize keeps tracking"
                  }
                >
                  <SegmentedControl
                    name="close-behavior"
                    value={settings.closeBehavior}
                    options={CLOSE_OPTIONS}
                    onChange={(closeBehavior) => update({ closeBehavior })}
                  />
                </SettingRow>
              </SettingGroup>
            </section>
          )}

          {activeSection === "tracking" && (
            <section className="flex flex-col gap-4">
              <SettingsSectionHeading
                title="Tracking & work"
                description="Choose when activity is captured and set the target used in work reports."
              />
              <TrackingHoursSetting settings={settings} update={update} />
              <SettingGroup title="Work hours">
                <SettingRow
                  title="Expected hours"
                  description="Your weekly work target. Calendar and My Timesheet use one fifth as the daily target."
                >
                  <Select
                    label="Expected hours per week"
                    value={settings.weeklyTargetHours}
                    options={targetOptions(settings.weeklyTargetHours)}
                    onChange={(weeklyTargetHours) =>
                      update({ weeklyTargetHours })
                    }
                  />
                </SettingRow>
                <SettingRow
                  title="Count toward Work Hours"
                  description="Choose which categories count as work rather than personal time"
                  status="not-implemented"
                />
              </SettingGroup>
            </section>
          )}

          {activeSection === "suggestions" && (
            <section className="flex flex-col gap-4">
              <SettingsSectionHeading
                title="Suggestions"
                description="Set everyday categorization and confident auto-accept behavior."
              />
              <SettingGroup title="Categories & AI">
                <SettingRow
                  title="Suggestion engine"
                  description={engineSummary(aiStatus)}
                >
                  <span className="shrink-0 font-mono text-[11px] text-fg-muted">
                    {aiStatus === null
                      ? ""
                      : aiStatus.calibrated
                        ? `${aiStatus.outcomes} reviewed`
                        : `learning · ${aiStatus.outcomes}/50 reviewed`}
                  </span>
                </SettingRow>
                <SettingRow
                  title="What to suggest"
                  description="Suggest a category for each entry, or a project too"
                >
                  <SegmentedControl
                    name="ai-suggest"
                    value={settings.aiSuggest}
                    options={SUGGEST_OPTIONS}
                    onChange={(aiSuggest) => update({ aiSuggest })}
                  />
                </SettingRow>
                <SettingRow
                  title="Auto-accept confident suggestions"
                  description={
                    aiStatus?.calibrated === false
                      ? "Starts after 50 reviewed suggestions; until then only rule matches auto-approve"
                      : "Approve entries whose every suggestion clears the threshold"
                  }
                >
                  <Toggle
                    checked={settings.autoAccept}
                    label="Auto-accept confident suggestions"
                    onChange={(autoAccept) => update({ autoAccept })}
                  />
                </SettingRow>
                <SettingBlock
                  title="Auto-accept threshold"
                  description="Minimum calibrated confidence for an entry to skip review"
                >
                  <ThresholdSlider
                    value={settings.autoAcceptPercent}
                    disabled={!settings.autoAccept}
                    metrics={metrics}
                    calibrated={aiStatus?.calibrated ?? false}
                    onChange={saveThreshold}
                  />
                </SettingBlock>
                <CustomInstructions
                  value={settings.aiCustomPrompt}
                  onSave={(aiCustomPrompt) => update({ aiCustomPrompt })}
                />
              </SettingGroup>
            </section>
          )}

          {activeSection === "data" && (
            <section className="flex flex-col gap-4">
              <SettingsSectionHeading
                title="Data & storage"
                description="Review where OpenRize stores local information and how long automatic activity is retained."
              />
              <SettingGroup title="Local data">
                <SettingBlock
                  title="Storage location"
                  description="Where your trackers, activity history, and preferences live"
                >
                  {storage === null ? (
                    <p className="text-[12px] text-fg-muted">Locating…</p>
                  ) : (
                    <div className="flex flex-col gap-3">
                      <PathLine label="Data folder" path={storage.dataDir} />
                      <div className="flex flex-col gap-1.5">
                        <FileLine
                          label="Database"
                          path={storage.databaseFile}
                        />
                        <FileLine
                          label="Preferences"
                          path={storage.configFile}
                        />
                      </div>
                      <button
                        type="button"
                        onClick={openStorage}
                        className="self-start rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] text-fg-muted hover:bg-surface-strong"
                      >
                        Open folder in Finder
                      </button>
                    </div>
                  )}
                </SettingBlock>
                <SettingRow
                  title="Keep activity until"
                  description="Delete automatic activity history older than this. Manual trackers are never touched."
                >
                  <Select
                    label="Keep activity until"
                    value={settings.retentionDays}
                    options={RETENTION_OPTIONS}
                    onChange={(retentionDays) => update({ retentionDays })}
                  />
                </SettingRow>
              </SettingGroup>
            </section>
          )}

          {activeSection === "notifications" && (
            <section className="flex flex-col gap-4">
              <SettingsSectionHeading
                title="Notifications"
                description="Notification preferences will appear here when reminders are available."
              />
              <SettingGroup title="Reminders">
                <SettingRow
                  title="Long-run reminders"
                  description="Ping me when a tracker has been running unusually long"
                  status="not-implemented"
                />
              </SettingGroup>
            </section>
          )}

          {activeSection === "advanced" && (
            <section className="flex flex-col gap-4">
              <SettingsSectionHeading
                title="Advanced settings"
                description="AI effectiveness, personal-model maintenance, and Battery & Energy. Everyday tracking and suggestion controls remain in their own sections."
              />
              <SettingGroup title="AI effectiveness">
                <div className="flex items-center justify-between gap-4 border-b border-line px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-[13px] font-medium text-fg">
                      How suggestions are doing
                    </div>
                    <div className="text-[12px] leading-relaxed text-fg-muted">
                      Measured on your own reviews, on this Mac
                    </div>
                  </div>
                  <SegmentedControl
                    name="ai-metrics-range"
                    value={metricDays}
                    options={METRIC_RANGES}
                    onChange={setMetricDays}
                  />
                </div>
                <div className="border-b border-line px-4 py-3">
                  {metrics === null ? (
                    <p className="text-[12px] text-fg-muted">Loading…</p>
                  ) : (
                    <AiEffectiveness metrics={metrics} status={aiStatus} />
                  )}
                </div>
                <SettingBlock
                  title="Personal model"
                  description="Classifiers trained on your approved entries"
                >
                  {metrics === null ? (
                    <p className="text-[12px] text-fg-muted">Loading…</p>
                  ) : (
                    <PersonalModels
                      metrics={metrics}
                      status={aiStatus}
                      onRetrain={retrain}
                    />
                  )}
                </SettingBlock>
                <SettingRow
                  title="Reset learned data"
                  description="Forget personal models, calibration, and the similar-entry index. Entries, categories, projects, and rules stay."
                >
                  <button
                    type="button"
                    onClick={() => setConfirmReset(true)}
                    className="shrink-0 rounded-lg border border-danger/40 bg-danger-soft px-3 py-1.5 text-[12px] font-medium text-danger hover:bg-danger/20"
                  >
                    Reset…
                  </button>
                </SettingRow>
              </SettingGroup>

              <SettingGroup title="Battery & Energy">
                <BatteryEnergyMonitor
                  summary={energySummary}
                  loading={energyLoading}
                  selectedDays={energyDays}
                  onRangeChange={setEnergyDays}
                  onReset={resetEnergyHistory}
                />
              </SettingGroup>
              {confirmReset && (
                <ConfirmDialog
                  title="Reset learned data?"
                  body="OpenRize forgets its personal models, calibration, and the index of similar entries. Your entries, categories, projects, and rules are kept. Suggestions start over from Apple's on-device model and rules, and confidence is capped at 90% again until you review 50 more."
                  confirmLabel="Reset learned data"
                  onConfirm={resetLearned}
                  onCancel={() => setConfirmReset(false)}
                />
              )}
            </section>
          )}
        </div>
      </div>
    </main>
  );
}
