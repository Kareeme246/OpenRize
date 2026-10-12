import { useState } from "react";
import { formatDuration, formatTime } from "../lib/format";
import type { TimeEntry } from "../lib/types";
import { FIELD } from "./Page";
import { Field, Sheet } from "./Sheet";

/** Neither half of a split may be shorter than this. */
export const MIN_HALF_MS = 60_000;

interface SplitDialogProps {
  entry: TimeEntry;
  /** Where the cut starts out: the picked point, or the smart split point. */
  initialAt: number;
  /** The entry's end; for the live entry, now. */
  endsAt: number;
  error: string | null;
  onSplit: (at: number) => void;
  onCancel: () => void;
}

/** `HH:MM` local time of an epoch ms. */
function clock(ms: number): string {
  return new Date(ms).toTimeString().slice(0, 5);
}

/**
 * The time `HH:MM` names inside the entry: on its start day, or the next day
 * for an entry that runs past midnight.
 */
function timeInside(entry: TimeEntry, value: string): number {
  const [hours, minutes] = value.split(":").map(Number);
  const date = new Date(entry.startedAt);
  date.setHours(hours, minutes, 0, 0);
  if (date.getTime() <= entry.startedAt) date.setDate(date.getDate() + 1);
  return date.getTime();
}

/**
 * Confirms a split: where to cut (adjustable to the minute) and the two
 * entries that result. Splitting the live entry closes its first half and
 * keeps recording into the second.
 */
export function SplitDialog({
  entry,
  initialAt,
  endsAt,
  error,
  onSplit,
  onCancel,
}: SplitDialogProps) {
  // The picked point keeps its seconds until the user edits the time.
  const [value, setValue] = useState(clock(initialAt));
  const at = value === clock(initialAt) ? initialAt : timeInside(entry, value);
  const live = entry.status === "building";
  const valid =
    Number.isFinite(at) &&
    at - entry.startedAt >= MIN_HALF_MS &&
    endsAt - at >= MIN_HALF_MS;

  const halves = [
    { label: "First", start: entry.startedAt, end: at, live: false },
    { label: "Second", start: at, end: endsAt, live },
  ];

  return (
    <Sheet
      title="Split entry"
      submitLabel="Split"
      width="sm"
      canSubmit={valid}
      error={error}
      onSubmit={() => onSplit(at)}
      onClose={onCancel}
    >
      <Field
        label="Split at"
        htmlFor="split-at"
        hint={
          !valid &&
          `Pick a time between ${formatTime(entry.startedAt + MIN_HALF_MS)} and ${formatTime(endsAt - MIN_HALF_MS)}.`
        }
      >
        <input
          id="split-at"
          type="time"
          value={value}
          onChange={(event) => setValue(event.target.value)}
          className={FIELD}
          required
        />
      </Field>
      <ol className="divide-y divide-line overflow-hidden rounded-lg border border-line">
        {halves.map((half) => (
          <li
            key={half.label}
            className={`flex items-center gap-3 px-3 py-2 ${valid ? "" : "opacity-40"}`}
          >
            <span className="w-12 shrink-0 font-medium text-[11px] text-fg-faint uppercase tracking-wider">
              {half.label}
            </span>
            <span className="min-w-0 flex-1 truncate text-fg tabular-nums">
              {formatTime(half.start)} –{" "}
              {half.live ? "now" : formatTime(half.end)}
            </span>
            {half.live && (
              <span className="flex shrink-0 items-center gap-1 font-semibold text-[10.5px] text-danger">
                <span className="recording-dot size-1.5 rounded-full bg-danger" />
                Recording
              </span>
            )}
            <span className="shrink-0 text-fg-soft tabular-nums">
              {valid ? formatDuration(half.end - half.start) : "–"}
            </span>
          </li>
        ))}
      </ol>
    </Sheet>
  );
}
