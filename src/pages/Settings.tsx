import { openPath } from "@tauri-apps/plugin-opener";
import { type ReactNode, useCallback, useState } from "react";
import {
  AiEffectiveness,
  PersonalModels,
  ThresholdSlider,
} from "../components/AiLearning";
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
import { useSettings } from "../hooks/useSettings";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import {
  ACCENT_ORDER,
  ACCENTS,
  type Accent,
  type AiSuggest,
  type CloseBehavior,
  type Theme,
} from "../lib/settings";
import type { AiStatus } from "../lib/types";

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

const SUGGEST_OPTIONS: SegmentedOption<AiSuggest>[] = [
  { value: "category", label: "Category" },
  { value: "categoryProject", label: "Category + Project" },
];

const METRIC_RANGES: SegmentedOption<number>[] = [
  { value: 7, label: "7d" },
  { value: 30, label: "30d" },
  { value: 90, label: "90d" },
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
        className="mt-2 w-full resize-y rounded-lg border border-line bg-surface px-2.5 py-2 text-[12px] text-fg outline-hidden placeholder:text-fg-faint focus:border-accent"
        placeholder="No custom instructions"
      />
    </SettingBlock>
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

  const trayOff = !settings.trayEnabled;
  const problem = error ?? openError ?? aiError ?? metricsError;

  return (
    <main className="flex h-full min-h-0 flex-1 flex-col gap-5 overflow-y-auto overscroll-contain p-5.5">
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

      <SettingGroup title="Categories & AI">
        <SettingRow
          title="Suggestion engine"
          description={engineSummary(aiStatus)}
        >
          <span className="shrink-0 font-mono text-[10.5px] text-fg-faint">
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

      <SettingGroup title="AI effectiveness">
        <div className="flex items-center justify-between gap-4 border-b border-line px-4 py-3">
          <div className="min-w-0">
            <div className="text-[13px] font-medium text-fg">
              How suggestions are doing
            </div>
            <div className="text-[11.5px] text-fg-faint">
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
            <p className="text-[11.5px] text-fg-faint">Loading…</p>
          ) : (
            <AiEffectiveness metrics={metrics} status={aiStatus} />
          )}
        </div>
        <SettingBlock
          title="Personal model"
          description="Classifiers trained on your approved entries"
        >
          {metrics === null ? (
            <p className="text-[11.5px] text-fg-faint">Loading…</p>
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
          description="Forget the personal models, calibration, and similar-entry index. Entries, categories, projects, and rules stay."
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
      {confirmReset && (
        <ConfirmDialog
          title="Reset learned data?"
          body="OpenRize forgets its personal models, calibration, and the index of similar entries. Your entries, categories, projects, and rules are kept. Suggestions start over from Apple's on-device model and your rules, and confidence is capped at 90% again until you review 50 more."
          confirmLabel="Reset learned data"
          onConfirm={resetLearned}
          onCancel={() => setConfirmReset(false)}
        />
      )}

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
          description="Your work week. The Calendar and My Timesheet measure against it; a day's target is a fifth of it."
        >
          <Select
            label="Expected hours per week"
            value={settings.weeklyTargetHours}
            options={targetOptions(settings.weeklyTargetHours)}
            onChange={(weeklyTargetHours) => update({ weeklyTargetHours })}
          />
        </SettingRow>
        <SettingRow
          title="Count toward Work Hours"
          description="Choose which categories count as work rather than personal time"
          status="not-implemented"
        />
      </SettingGroup>
    </main>
  );
}
