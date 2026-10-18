import { type FocusEvent, type KeyboardEvent, useRef, useState } from "react";
import { formatDuration, type Timer, type TimerSegment } from "../lib/timers";
import { FIELD } from "./Page";
import { Sheet } from "./Sheet";

interface TimerSegmentsSheetProps {
  timer: Timer;
  /** The running stretch so far, shown read-only until it is paused. */
  runningMs: number;
  total: number;
  onAdd: (durationMs: number) => void;
  onUpdate: (segmentId: number, durationMs: number) => void;
  onDelete: (segmentId: number) => void;
  onClose: () => void;
}

const UNITS = [3_600_000, 60_000, 1_000] as const;
const LABELS = ["h", "m", "s"] as const;

function split(ms: number): string[] {
  const total = Math.floor(ms / 1000);
  return [
    Math.floor(total / 3600),
    Math.floor((total % 3600) / 60),
    total % 60,
  ].map(String);
}

/** Blank counts as 0; null when any part is not a whole number of 0 or more. */
function join(parts: string[]): number | null {
  const numbers = parts.map((part) => (part.trim() === "" ? 0 : Number(part)));
  if (!numbers.every((value) => Number.isInteger(value) && value >= 0)) {
    return null;
  }
  return numbers.reduce((sum, value, index) => sum + value * UNITS[index], 0);
}

interface DurationFieldsProps {
  parts: string[];
  label: string;
  onChange: (parts: string[]) => void;
}

function DurationFields({ parts, label, onChange }: DurationFieldsProps) {
  return (
    <div className="flex items-center gap-1">
      {parts.map((part, index) => (
        <label
          key={LABELS[index]}
          className="flex items-center gap-0.5 text-fg-faint"
        >
          <input
            type="number"
            min={0}
            step={1}
            inputMode="numeric"
            aria-label={`${label} ${LABELS[index] === "h" ? "hours" : LABELS[index] === "m" ? "minutes" : "seconds"}`}
            value={part}
            onChange={(event) =>
              onChange(
                parts.map((old, at) =>
                  at === index ? event.target.value : old,
                ),
              )
            }
            className={`${FIELD} w-14 px-1.5 text-right font-mono tabular-nums`}
          />
          {LABELS[index]}
        </label>
      ))}
    </div>
  );
}

function SegmentRow({
  segment,
  onUpdate,
  onDelete,
}: {
  segment: TimerSegment;
  onUpdate: (segmentId: number, durationMs: number) => void;
  onDelete: (segmentId: number) => void;
}) {
  const [parts, setParts] = useState(() => split(segment.durationMs));
  const ref = useRef<HTMLDivElement>(null);
  const label = "Segment";

  const commit = (): void => {
    const next = join(parts);
    if (next === null || next === 0) {
      setParts(split(segment.durationMs));
    } else if (next !== segment.durationMs) {
      onUpdate(segment.id, next);
    }
  };

  // The form around the rows would submit (close) on Enter; commit instead.
  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    commit();
  };
  const onBlur = (event: FocusEvent): void => {
    if (!ref.current?.contains(event.relatedTarget as Node | null)) commit();
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: groups the fields so blur commits once focus leaves the row; the inputs inside are the interactive elements
    <div
      ref={ref}
      onKeyDown={onKeyDown}
      onBlur={onBlur}
      className="flex items-center justify-between gap-2"
    >
      <span className="min-w-0 truncate text-fg-muted">
        {new Date(segment.recordedAt).toLocaleString([], {
          month: "short",
          day: "numeric",
          hour: "numeric",
          minute: "2-digit",
        })}
      </span>
      <div className="flex items-center gap-1.5">
        <DurationFields parts={parts} label={label} onChange={setParts} />
        <button
          type="button"
          aria-label="Remove segment"
          onClick={() => onDelete(segment.id)}
          className="rounded p-1 text-fg-faint hover:bg-surface hover:text-danger"
        >
          ✕
        </button>
      </div>
    </div>
  );
}

/**
 * A stopwatch's total is the sum of its segments, so editing the time means
 * adding, changing or removing a segment. Pausing records one automatically.
 */
export function TimerSegmentsSheet({
  timer,
  runningMs,
  total,
  onAdd,
  onUpdate,
  onDelete,
  onClose,
}: TimerSegmentsSheetProps) {
  const [parts, setParts] = useState(["0", "0", "0"]);
  const added = join(parts);
  const canAdd = added !== null && added > 0;

  const add = (): void => {
    if (added === null || added === 0) return;
    onAdd(added);
    setParts(["0", "0", "0"]);
  };

  return (
    <Sheet
      title={`Edit time - ${timer.label}`}
      submitLabel="Done"
      onSubmit={onClose}
      onClose={onClose}
      width="sm"
    >
      <div className="flex items-baseline justify-between">
        <span className="font-medium text-[11.5px] text-fg-soft">Segments</span>
        <span className="font-mono text-fg-strong tabular-nums">
          Total {formatDuration(total)}
        </span>
      </div>
      <div className="space-y-2">
        {timer.segments.length === 0 && runningMs === 0 && (
          <p className="text-fg-faint">No time yet. Add a segment below.</p>
        )}
        {timer.segments.map((segment) => (
          <SegmentRow
            key={`${segment.id}-${segment.durationMs}`}
            segment={segment}
            onUpdate={onUpdate}
            onDelete={onDelete}
          />
        ))}
        {timer.startedAt !== null && (
          <div className="flex items-center justify-between text-fg-faint">
            <span>Running now</span>
            <span className="font-mono tabular-nums">
              {formatDuration(runningMs)}
            </span>
          </div>
        )}
      </div>
      <div className="border-line border-t pt-3.5">
        <div className="mb-1.5 font-medium text-[11.5px] text-fg-soft">
          Add a segment
        </div>
        {/* biome-ignore lint/a11y/noStaticElementInteractions: Enter in the fields adds the segment instead of submitting the sheet */}
        <div
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            add();
          }}
          className="flex items-center justify-between gap-2"
        >
          <DurationFields parts={parts} label="New" onChange={setParts} />
          <button
            type="button"
            disabled={!canAdd}
            onClick={add}
            className="rounded-md border border-line bg-surface px-3 py-1.5 text-[12px] text-fg hover:bg-surface-strong disabled:opacity-40"
          >
            Add
          </button>
        </div>
      </div>
    </Sheet>
  );
}
