import { openPath } from "@tauri-apps/plugin-opener";
import { type ReactNode, useState } from "react";
import {
  SegmentedControl,
  type SegmentedOption,
  Toggle,
} from "../components/SegmentedControl";
import { type Status, StatusBadge } from "../components/StatusBadge";
import { useSettings } from "../hooks/useSettings";
import { describeError } from "../lib/api";
import {
  ACCENT_ORDER,
  ACCENTS,
  type Accent,
  type CloseBehavior,
  type Theme,
} from "../lib/settings";

const THEME_OPTIONS: SegmentedOption<Theme>[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
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
    <div className="flex items-center justify-between gap-4 border-b border-line px-4 py-3 last:border-b-0">
      <div className="min-w-0">
        <div className="text-[13px] font-medium text-fg">{title}</div>
        <div className="text-[11.5px] text-fg-faint">{description}</div>
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
    <div className="border-b border-line px-4 py-3 last:border-b-0">
      <div className="text-[13px] font-medium text-fg">{title}</div>
      <div className="text-[11.5px] text-fg-faint">{description}</div>
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
      <h2 className="px-1 font-mono text-[10px] uppercase tracking-wider text-fg-faint">
        {title}
      </h2>
      <div className="rounded-xl border border-line bg-linear-to-b from-surface to-transparent">
        {children}
      </div>
    </section>
  );
}

function PathLine({ label, path }: { label: string; path: string }) {
  return (
    <div className="flex flex-col gap-1">
      <span className="font-mono text-[10px] uppercase tracking-wider text-fg-faint">
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
      <span className="shrink-0 text-fg-soft">{label}</span>
      <span
        title={path}
        className="min-w-0 truncate font-mono text-[11px] text-fg-faint"
      >
        {path}
      </span>
    </div>
  );
}

/** A native dropdown, styled to match the segmented controls beside it. */
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
    <div className="relative shrink-0">
      <select
        aria-label={label}
        value={value}
        onChange={(event) => onChange(Number(event.target.value))}
        className="appearance-none rounded-lg border border-line bg-surface py-1.5 pl-2.5 pr-7 text-[12px] text-fg-muted outline-none transition-colors hover:bg-surface-strong focus:border-accent/40"
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
      <svg
        viewBox="0 0 24 24"
        aria-hidden="true"
        fill="none"
        stroke="currentColor"
        strokeWidth={2.2}
        strokeLinecap="round"
        strokeLinejoin="round"
        className="pointer-events-none absolute right-2 top-1/2 size-3 -translate-y-1/2 text-fg-faint"
      >
        <path d="m6 9 6 6 6-6" />
      </svg>
    </div>
  );
}

export function Settings() {
  const { settings, storage, error, update } = useSettings();
  const [openError, setOpenError] = useState<string | null>(null);

  const openStorage = (): void => {
    if (storage === null) return;
    setOpenError(null);
    openPath(storage.dataDir).catch((cause: unknown) =>
      setOpenError(describeError(cause)),
    );
  };

  const trayOff = !settings.trayEnabled;
  const problem = error ?? openError;

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <header>
        <h1 className="text-[15px] font-semibold">Settings</h1>
        <div className="font-mono text-[10.5px] text-fg-faint">
          ⌘, opens this · changes save automatically
        </div>
      </header>

      {problem !== null && (
        <p
          role="alert"
          className="rounded-[10px] border border-danger/40 bg-danger-soft px-3.5 py-2.5 text-[13px] text-danger"
        >
          {problem}
        </p>
      )}

      <SettingGroup title="Appearance">
        <SettingRow title="Theme" description="Dark, light, or follow macOS">
          <SegmentedControl
            name="theme"
            value={settings.theme}
            options={THEME_OPTIONS}
            onChange={(theme) => update({ theme })}
          />
        </SettingRow>
        <SettingRow
          title="Accent colour"
          description="Highlights, active states, and running timers across the app"
        >
          <SegmentedControl
            name="accent"
            value={settings.accent}
            options={ACCENT_OPTIONS}
            onChange={(accent) => update({ accent })}
          />
        </SettingRow>
      </SettingGroup>

      <SettingGroup title="General">
        <SettingRow
          title="Launch at login"
          description="Start OpenRize when you log in"
          status="not-implemented"
        />
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

      <SettingGroup title="Notifications">
        <SettingRow
          title="Long-run reminders"
          description="Ping me when a tracker has been running unusually long"
          status="not-implemented"
        />
      </SettingGroup>

      <SettingGroup title="Data">
        <SettingBlock
          title="Storage location"
          description="Where your trackers, activity history, and preferences live"
        >
          {storage === null ? (
            <p className="text-[11.5px] text-fg-faint">Locating…</p>
          ) : (
            <div className="flex flex-col gap-3">
              <PathLine label="Data folder" path={storage.dataDir} />
              <div className="flex flex-col gap-1.5">
                <FileLine label="Database" path={storage.databaseFile} />
                <FileLine label="Preferences" path={storage.configFile} />
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

      <SettingGroup title="Work hours">
        <SettingRow
          title="Expected hours"
          description="Your baseline work week, used for utilization and capacity metrics"
          status="not-implemented"
        />
        <SettingRow
          title="Count toward Work Hours"
          description="Choose which categories count as work rather than personal time"
          status="not-implemented"
        />
      </SettingGroup>
    </main>
  );
}
