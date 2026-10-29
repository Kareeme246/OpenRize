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
import { localDateTimeString, parseLocalDateTime } from "../lib/dates";
import { formatShortDate, formatTime } from "../lib/format";
import {
  formatDuration,
  type SegmentEdit,
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
  onAdd: (startedAt: number, endedAt: number) => void;
  onUpdate: (segmentId: number, edit: SegmentEdit) => void;
  onDelete: (segmentId: number) => void;
  onClose: () => void;
}

const BACKWARDS = "A segment cannot end before it starts.";

/** A new segment starts as the last hour, ending now. */
function lastHour(): [string, string] {
  const end = Date.now();
  return [localDateTimeString(end - 3_600_000), localDateTimeString(end)];
}

interface SpanFieldsProps {
  start: string;
  end: string;
  label: string;
  onStart: (value: string) => void;
  onEnd: (value: string) => void;
}

/** A segment's start and end, as a time entry's are. */
function SpanFields({ start, end, label, onStart, onEnd }: SpanFieldsProps) {
  return (
    <div className="flex items-center gap-1.5 text-fg-faint">
      <input
        type="datetime-local"
        step={1}
        aria-label={`${label} start`}
        value={start}
        onChange={(event) => onStart(event.target.value)}
        className={`${FIELD} w-[11.5rem] px-1.5 font-mono tabular-nums`}
      />
      <span aria-hidden="true">to</span>
      <input
        type="datetime-local"
        step={1}
        aria-label={`${label} end`}
        value={end}
        onChange={(event) => onEnd(event.target.value)}
        className={`${FIELD} w-[11.5rem] px-1.5 font-mono tabular-nums`}
      />
    </div>
  );
}

function SegmentRow({
  segment,
  onUpdate,
  onDelete,
  onInvalid,
}: {
  segment: TimerSegment;
  onUpdate: (segmentId: number, edit: SegmentEdit) => void;
  onDelete: (segmentId: number) => void;
  onInvalid: (message: string | null) => void;
}) {
  const savedStart = localDateTimeString(segment.startedAt);
  const savedEnd = localDateTimeString(segment.endedAt);
  const [start, setStart] = useState(savedStart);
  const [end, setEnd] = useState(savedEnd);
  const ref = useRef<HTMLDivElement>(null);
  const label = "Segment";

  const revert = (): void => {
    setStart(savedStart);
    setEnd(savedEnd);
  };

  // Only a boundary that was touched is sent, so the seconds and milliseconds
  // of the one left alone survive an edit of the other.
  const commit = (): void => {
    const startedAt = start === savedStart ? null : parseLocalDateTime(start);
    const endedAt = end === savedEnd ? null : parseLocalDateTime(end);
    if (
      (start !== savedStart && startedAt === null) ||
      (end !== savedEnd && endedAt === null)
    ) {
      revert();
      return;
    }
    if (startedAt === null && endedAt === null) return;
    if ((endedAt ?? segment.endedAt) < (startedAt ?? segment.startedAt)) {
      onInvalid(BACKWARDS);
      revert();
      return;
    }
    onInvalid(null);
    onUpdate(segment.id, {
      ...(startedAt === null ? {} : { startedAt }),
      ...(endedAt === null ? {} : { endedAt }),
    });
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
      <SpanFields
        start={start}
        end={end}
        label={label}
        onStart={setStart}
        onEnd={setEnd}
      />
      <div className="flex items-center gap-1.5">
        <span className="w-14 text-right font-mono text-fg-muted tabular-nums">
          {formatDuration(segment.durationMs)}
        </span>
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
  const [[newStart, newEnd], setNewSpan] = useState(lastHour);
  const [states, setStates] = useState<TimerSavedState[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [segmentError, setSegmentError] = useState<string | null>(null);
  const addedStart = parseLocalDateTime(newStart);
  const addedEnd = parseLocalDateTime(newEnd);
  const backwards =
    addedStart !== null && addedEnd !== null && addedEnd < addedStart;
  const canAdd = addedStart !== null && addedEnd !== null && !backwards;

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
    if (addedStart === null || addedEnd === null || backwards) return;
    onAdd(addedStart, addedEnd);
    setSegmentError(null);
    setNewSpan(lastHour());
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
      error={error ?? segmentError ?? (backwards ? BACKWARDS : null)}
      cancellable={false}
      width="lg"
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
          <p className="text-fg-faint">
            No time yet. Add a segment below with a start and an end.
          </p>
        )}
        {timer.segments.map((segment) => (
          <SegmentRow
            key={`${segment.id}-${segment.startedAt}-${segment.endedAt}`}
            segment={segment}
            onUpdate={onUpdate}
            onDelete={onDelete}
            onInvalid={setSegmentError}
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
            <SpanFields
              start={newStart}
              end={newEnd}
              label="New segment"
              onStart={(value) => setNewSpan([value, newEnd])}
              onEnd={(value) => setNewSpan([newStart, value])}
            />
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
