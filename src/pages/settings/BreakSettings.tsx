import { useState } from "react";
import {
  SegmentedControl,
  type SegmentedOption,
  Toggle,
} from "../../components/SegmentedControl";
import { Tooltip } from "../../components/Tooltip";
import { useBreaks } from "../../hooks/useBreaks";
import { useSettings } from "../../hooks/useSettings";
import * as api from "../../lib/api";
import { SNOOZE_CHOICES } from "../../lib/breaks";
import { formatTime } from "../../lib/format";
import {
  type BreakSettings,
  MAX_BREAK_MESSAGE_CHARS,
  MAX_SCHEDULED_BREAKS,
  type ScheduledBreak,
  WEEKDAY_KEYS,
  WEEKDAYS_MON_FRI,
  type Weekdays,
} from "../../lib/settings";
import { Select, SettingBlock, SettingGroup, SettingRow } from "./SettingParts";

const SNOOZE_OPTIONS: SegmentedOption<number>[] = SNOOZE_CHOICES.map(
  (minutes) => ({ value: minutes, label: `${minutes} min` }),
);

const DAY_LETTERS: Record<keyof Weekdays, string> = {
  mon: "M",
  tue: "T",
  wed: "W",
  thu: "T",
  fri: "F",
  sat: "S",
  sun: "S",
};

const DAY_NAMES: Record<keyof Weekdays, string> = {
  mon: "Monday",
  tue: "Tuesday",
  wed: "Wednesday",
  thu: "Thursday",
  fri: "Friday",
  sat: "Saturday",
  sun: "Sunday",
};

const SCHEDULE_LENGTHS = [5, 10, 15, 20, 30, 45, 60, 90, 120];

function minutesLabel(minutes: number): string {
  if (minutes < 60) return `${minutes} min`;
  const rest = minutes % 60;
  return `${Math.floor(minutes / 60)}h${rest === 0 ? "" : ` ${rest}m`}`;
}

/** Sorted options that always include the current value. */
function minuteOptions(
  values: number[],
  current: number,
): { value: number; label: string }[] {
  return [...new Set([...values, current])]
    .sort((a, b) => a - b)
    .map((value) => ({ value, label: minutesLabel(value) }));
}

const WORK_STEPS = Array.from({ length: 34 }, (_, index) => 15 + index * 5);
const BREAK_STEPS = [1, 2, 3, 5, 10, 15, 20, 30, 45, 60];

/** Saved on blur, so typing doesn't rewrite settings.json per keystroke. */
function TextSetting({
  label,
  value,
  placeholder,
  maxLength,
  className = "",
  onSave,
}: {
  label: string;
  value: string;
  placeholder: string;
  maxLength: number;
  className?: string;
  onSave: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const [focused, setFocused] = useState(false);
  return (
    <input
      type="text"
      aria-label={label}
      value={focused ? draft : value}
      maxLength={maxLength}
      placeholder={placeholder}
      onFocus={() => {
        setDraft(value);
        setFocused(true);
      }}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => {
        setFocused(false);
        if (draft !== value) onSave(draft);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") event.currentTarget.blur();
      }}
      className={`h-8 rounded-lg border border-line bg-surface px-2.5 text-[12px] text-fg outline-hidden placeholder:text-fg-muted focus:border-accent ${className}`}
    />
  );
}

function DayChips({
  days,
  onChange,
}: {
  days: Weekdays;
  onChange: (days: Weekdays) => void;
}) {
  return (
    <fieldset className="m-0 flex gap-1 border-0 p-0">
      <legend className="sr-only">Repeats on</legend>
      {WEEKDAY_KEYS.map((key) => (
        <Tooltip key={key} content={DAY_NAMES[key]}>
          <button
            type="button"
            aria-pressed={days[key]}
            aria-label={DAY_NAMES[key]}
            onClick={() => onChange({ ...days, [key]: !days[key] })}
            className={`size-7 rounded-md border text-[11px] font-semibold transition-colors ${
              days[key]
                ? "border-accent/40 bg-accent-soft text-fg-strong"
                : "border-line bg-surface text-fg-soft hover:bg-surface-strong"
            }`}
          >
            {DAY_LETTERS[key]}
          </button>
        </Tooltip>
      ))}
    </fieldset>
  );
}

function ScheduleEditor({
  schedule,
  onChange,
  onDelete,
}: {
  schedule: ScheduledBreak;
  onChange: (patch: Partial<ScheduledBreak>) => void;
  onDelete: () => void;
}) {
  return (
    <div className="setting-block flex flex-col gap-3 border-b border-line px-4 py-3 last:border-b-0">
      <div className="flex items-center gap-3">
        <TextSetting
          label="Break name"
          value={schedule.label}
          placeholder="Lunch"
          maxLength={40}
          className="min-w-0 flex-1"
          onSave={(label) => onChange({ label })}
        />
        <Toggle
          checked={schedule.enabled}
          label={`${schedule.label} enabled`}
          onChange={(enabled) => onChange({ enabled })}
        />
        <Tooltip content="Delete">
          <button
            type="button"
            aria-label={`Delete ${schedule.label}`}
            onClick={onDelete}
            className="grid size-7 shrink-0 place-items-center rounded-md text-fg-soft transition-colors hover:bg-danger-soft hover:text-danger"
          >
            <svg
              viewBox="0 0 24 24"
              className="size-3.5"
              fill="none"
              stroke="currentColor"
              strokeWidth={1.9}
              strokeLinecap="round"
              strokeLinejoin="round"
              aria-hidden
            >
              <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />
            </svg>
          </button>
        </Tooltip>
      </div>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <label className="flex items-center gap-2 text-[12px] text-fg-muted">
          At
          <input
            type="time"
            value={schedule.at}
            onChange={(event) => onChange({ at: event.target.value })}
            className="h-7 rounded-md border border-line bg-surface px-2 py-1 text-[12px] text-fg outline-hidden focus:border-accent"
          />
        </label>
        <div className="flex items-center gap-2 text-[12px] text-fg-muted">
          For
          <Select
            label={`${schedule.label} length`}
            value={schedule.minutes}
            options={minuteOptions(SCHEDULE_LENGTHS, schedule.minutes)}
            onChange={(minutes) => onChange({ minutes })}
          />
        </div>
        <DayChips
          days={schedule.days}
          onChange={(days) => onChange({ days })}
        />
      </div>
    </div>
  );
}

function newSchedule(): ScheduledBreak {
  return {
    id: "",
    label: "Lunch",
    at: "12:30",
    minutes: 45,
    days: { ...WEEKDAYS_MON_FRI },
    enabled: true,
  };
}

/** Settings > Notifications: interval reminders and scheduled breaks. */
export function BreakSettingsGroups() {
  const { settings, update } = useSettings();
  const breaks = settings.breaks;
  const { state } = useBreaks();

  const change = (patch: Partial<BreakSettings>): void =>
    update({ breaks: { ...breaks, ...patch } });
  const changeSchedule = (
    index: number,
    patch: Partial<ScheduledBreak>,
  ): void =>
    change({
      schedules: breaks.schedules.map((schedule, position) =>
        position === index ? { ...schedule, ...patch } : schedule,
      ),
    });

  return (
    <>
      <SettingGroup title="Break reminders">
        <SettingRow
          title="Remind me to take breaks"
          description="A reminder appears in the top-right corner of your screen after a stretch of work, during your tracking hours"
        >
          <Toggle
            checked={breaks.enabled}
            label="Remind me to take breaks"
            onChange={(enabled) => change({ enabled })}
          />
        </SettingRow>
        {state.pausedUntil !== null && (
          <SettingRow
            title="Reminders are paused"
            description={`No reminders until ${formatTime(state.pausedUntil)}`}
          >
            <button
              type="button"
              onClick={() => void api.pauseBreakReminders(null)}
              className="rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] font-medium text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
            >
              Resume reminders
            </button>
          </SettingRow>
        )}
        {import.meta.env.DEV && (
          <SettingRow
            title="Sample reminder"
            description="Development builds only: raises a reminder now"
          >
            <button
              type="button"
              onClick={() => void api.devSampleBreakReminder()}
              className="rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] font-medium text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
            >
              Show sample
            </button>
          </SettingRow>
        )}
        {breaks.enabled && (
          <>
            <SettingRow
              title="Remind me after"
              description="Continuous work before a break is suggested"
            >
              <Select
                label="Remind me after"
                value={breaks.workMinutes}
                options={minuteOptions(WORK_STEPS, breaks.workMinutes)}
                onChange={(workMinutes) => change({ workMinutes })}
              />
            </SettingRow>
            <SettingRow
              title="Break length"
              description="How long the suggested break lasts"
            >
              <Select
                label="Break length"
                value={breaks.breakMinutes}
                options={minuteOptions(BREAK_STEPS, breaks.breakMinutes)}
                onChange={(breakMinutes) => change({ breakMinutes })}
              />
            </SettingRow>
            <SettingRow
              title="Default snooze"
              description="What the Snooze button does; its arrow offers 5, 10 or 15 minutes every time"
            >
              <SegmentedControl
                name="break-snooze"
                value={breaks.snoozeMinutes}
                options={SNOOZE_OPTIONS}
                onChange={(snoozeMinutes) => change({ snoozeMinutes })}
              />
            </SettingRow>
            <SettingBlock
              title="Reminder message"
              description="An optional extra line under the reminder. Leave empty to show nothing"
            >
              <TextSetting
                label="Reminder message"
                value={breaks.message}
                placeholder="No extra message"
                maxLength={MAX_BREAK_MESSAGE_CHARS}
                className="w-full"
                onSave={(message) => change({ message })}
              />
            </SettingBlock>
          </>
        )}
        <SettingRow
          title="Pause stopwatches during breaks"
          description="Running stopwatches pause when a break starts and resume when it ends"
        >
          <Toggle
            checked={breaks.pauseStopwatches}
            label="Pause stopwatches during breaks"
            onChange={(pauseStopwatches) => change({ pauseStopwatches })}
          />
        </SettingRow>
        <SettingRow
          title="Chime"
          description="A short sound when a reminder appears"
        >
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={() => void api.previewBreakChime()}
              className="rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] font-medium text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
            >
              Preview
            </button>
            <Toggle
              checked={breaks.chime}
              label="Chime"
              onChange={(chime) => change({ chime })}
            />
          </div>
        </SettingRow>
      </SettingGroup>

      <SettingGroup title="Scheduled breaks">
        {breaks.schedules.length === 0 && (
          <div className="border-b border-line px-4 py-3 text-[12px] leading-relaxed text-fg-muted">
            Fixed-time breaks, like lunch at 12:30. A scheduled break takes
            priority over a reminder that would land just before it.
          </div>
        )}
        {breaks.schedules.map((schedule, index) => (
          <ScheduleEditor
            key={schedule.id || index}
            schedule={schedule}
            onChange={(patch) => changeSchedule(index, patch)}
            onDelete={() =>
              change({
                schedules: breaks.schedules.filter(
                  (_, position) => position !== index,
                ),
              })
            }
          />
        ))}
        <div className="px-4 py-3">
          <button
            type="button"
            disabled={breaks.schedules.length >= MAX_SCHEDULED_BREAKS}
            onClick={() =>
              change({ schedules: [...breaks.schedules, newSchedule()] })
            }
            className="rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] font-medium text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg disabled:opacity-40"
          >
            Add scheduled break
          </button>
        </div>
      </SettingGroup>
    </>
  );
}
