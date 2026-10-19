import {
  type FocusEvent,
  type KeyboardEvent,
  type ReactNode,
  useEffect,
  useRef,
  useState,
} from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { formatShortDate, formatTime } from "../lib/format";
import {
  formatDuration,
  type Timer,
  type TimerSavedState,
  type TimerSegment,
} from "../lib/timers";
import { FIELD } from "./Page";
import { Sheet } from "./Sheet";

interface TimerInfoSheetProps {
  timer: Timer;
  /** The running stretch so far, shown read-only until it is paused. */
  runningMs: number;
  total: number;
  onRename: (label: string) => void;
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

/** `Oct 19, 4:58 PM`: a date with its time. */
function stamp(epochMs: number): string {
  return `${formatShortDate(epochMs)}, ${formatTime(epochMs)}`;
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-2 border-line border-t pt-3.5 first:border-t-0 first:pt-0">
      <h3 className="font-medium text-[11.5px] text-fg-soft">{title}</h3>
      {children}
    </section>
  );
}

/**
 * Everything about one stopwatch, and where it is edited: its name, what it
 * reads and since when, the segments its total is made of (pausing records
 * one; they can be added, changed or removed), and its saved states.
 */
export function TimerInfoSheet({
  timer,
  runningMs,
  total,
  onRename,
  onAdd,
  onUpdate,
  onDelete,
  onClose,
}: TimerInfoSheetProps) {
  const [name, setName] = useState(timer.label);
  const [parts, setParts] = useState(["0", "0", "0"]);
  const [states, setStates] = useState<TimerSavedState[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const added = join(parts);
  const canAdd = added !== null && added > 0;

  useEffect(() => {
    let cancelled = false;
    api
      .listTimerSavedStates(timer.id)
      .then((next) => {
        if (!cancelled) setStates(next);
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(describeError(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [timer.id]);

  // A blank name reverts, the same as renaming on the card.
  const commitName = (): void => {
    const next = name.trim();
    if (next.length > 0 && next !== timer.label) onRename(next);
    else setName(timer.label);
  };

  const add = (): void => {
    if (added === null || added === 0) return;
    onAdd(added);
    setParts(["0", "0", "0"]);
  };

  const running = timer.startedAt !== null;
  const facts: [string, string][] = [
    ["Elapsed", formatDuration(total)],
    [
      "Status",
      running && timer.startedAt !== null
        ? `Running since ${formatTime(timer.startedAt)}`
        : total > 0
          ? "Paused"
          : "Not started",
    ],
    ["Created", stamp(timer.createdAt)],
  ];

  return (
    <Sheet
      title={`Info - ${timer.label}`}
      submitLabel="Done"
      onSubmit={onClose}
      onClose={onClose}
      error={error}
      cancellable={false}
    >
      <Section title="Name">
        <input
          value={name}
          aria-label="Tracker name"
          onChange={(event) => setName(event.target.value)}
          onBlur={commitName}
          onKeyDown={(event) => {
            // Renames instead of submitting (closing) the sheet.
            if (event.key !== "Enter") return;
            event.preventDefault();
            commitName();
          }}
          className={FIELD}
        />
      </Section>

      <Section title="Details">
        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5">
          {facts.map(([label, value]) => (
            <div key={label} className="contents">
              <dt className="text-fg-faint">{label}</dt>
              <dd className="text-right font-mono text-fg-strong tabular-nums">
                {value}
              </dd>
            </div>
          ))}
        </dl>
      </Section>

      <Section title="Segments">
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
        {running && (
          <div className="flex items-center justify-between text-fg-faint">
            <span>Running now</span>
            <span className="font-mono tabular-nums">
              {formatDuration(runningMs)}
            </span>
          </div>
        )}
        {/* biome-ignore lint/a11y/noStaticElementInteractions: Enter in the fields adds the segment instead of submitting the sheet */}
        <div
          onKeyDown={(event) => {
            if (event.key !== "Enter") return;
            event.preventDefault();
            add();
          }}
          className="flex items-center justify-between gap-2 pt-1"
        >
          <span className="whitespace-nowrap text-fg-muted">Add a segment</span>
          <div className="flex items-center gap-1.5">
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
      </Section>

      <Section title="Saved states">
        {states === null ? (
          error === null && <p className="text-fg-faint">Loading</p>
        ) : states.length === 0 ? (
          <p className="text-fg-faint">
            No saved states yet. Choose Save state in the menu to record the
            time as it reads now.
          </p>
        ) : (
          <ul className="space-y-2">
            {states.map((state) => (
              <li
                key={state.id}
                className="flex items-center justify-between gap-2"
              >
                <span className="min-w-0 truncate text-fg-muted">
                  {stamp(state.savedAt)}
                  {state.label !== timer.label && (
                    <span className="text-fg-faint"> as {state.label}</span>
                  )}
                </span>
                <span className="font-mono text-fg-strong tabular-nums">
                  {formatDuration(state.elapsedMs)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </Sheet>
  );
}
