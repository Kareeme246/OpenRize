import { useCallback, useRef, useState } from "react";
import { elapsedMs, formatDuration, type Timer } from "../lib/timers";
import type { Category, Project } from "../lib/types";
import { useDismiss } from "../pages/timesheets/useDismiss";
import { AddTimeSheet } from "./AddTimeSheet";
import { ConfirmDialog } from "./ConfirmDialog";
import { HourRing } from "./HourRing";
import { TimerSavedStatesSheet } from "./TimerSavedStatesSheet";
import { TimerSegmentsSheet } from "./TimerSegmentsSheet";
import { Tooltip } from "./Tooltip";

interface StopwatchCardProps {
  timer: Timer;
  now: number;
  onStart: (id: string) => void;
  onPause: (id: string) => void;
  onReset: (id: string) => void;
  /** Resets quietly once "Add to calendar" has logged the time. */
  onClear: (id: string) => void;
  onRename: (id: string, label: string) => void;
  onAddSegment: (id: string, durationMs: number) => void;
  onUpdateSegment: (id: string, segmentId: number, durationMs: number) => void;
  onDeleteSegment: (id: string, segmentId: number) => void;
  onSaveState: (id: string) => void;
  onDelete: (id: string) => void;
  categories: Category[];
  projects: Project[];
}

export function StopwatchCard({
  timer,
  now,
  onStart,
  onPause,
  onReset,
  onClear,
  onRename,
  onAddSegment,
  onUpdateSegment,
  onDeleteSegment,
  onSaveState,
  onDelete,
  categories,
  projects,
}: StopwatchCardProps) {
  const running = timer.startedAt !== null;
  const elapsed = elapsedMs(timer, now);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(timer.label);
  /** Which confirmation is up, if any. Both destroy real accumulated time. */
  const [pending, setPending] = useState<"reset" | "delete" | null>(null);
  const [dialog, setDialog] = useState<"time" | "calendar" | "saved" | null>(
    null,
  );
  /** Frozen when "Add to calendar" opens, so the prefilled range holds still. */
  const [calendarRange, setCalendarRange] = useState<{
    start: number;
    end: number;
  } | null>(null);

  const commitRename = (): void => {
    const next = draft.trim();
    if (next.length > 0 && next !== timer.label) onRename(timer.id, next);
    else setDraft(timer.label);
    setEditing(false);
  };

  const startRename = (): void => {
    setDraft(timer.label);
    setEditing(true);
  };

  const openCalendar = (): void => {
    const end = Date.now();
    const midnight = new Date(end).setHours(0, 0, 0, 0);
    setCalendarRange({ start: Math.max(midnight, end - elapsed), end });
    setDialog("calendar");
  };

  return (
    <section
      id={`timer-${timer.id}`}
      tabIndex={-1}
      aria-label={`Stopwatch ${timer.label}`}
      className={`shape-bleed-table relative flex min-w-0 flex-col items-center gap-3 rounded-xl border border-settings-card-border bg-settings-card p-4 outline-hidden focus:ring-2 focus:ring-accent ${running ? "border-l-4 border-l-accent" : ""}`}
    >
      <TimerMenu
        label={timer.label}
        canAddToCalendar={elapsed >= 60_000}
        onSaveState={() => onSaveState(timer.id)}
        onShowSavedStates={() => setDialog("saved")}
        onRename={startRename}
        onEditTime={() => setDialog("time")}
        onAddToCalendar={openCalendar}
        onReset={() => setPending("reset")}
        onDelete={() => setPending("delete")}
      />

      <div className="flex w-full min-w-0 items-center justify-center gap-1.5 px-6">
        {editing ? (
          <input
            // biome-ignore lint/a11y/noAutofocus: intentional — focuses the rename input the moment editing starts
            autoFocus
            value={draft}
            aria-label="Tracker name"
            onChange={(event) => setDraft(event.target.value)}
            onBlur={commitRename}
            onKeyDown={(event) => {
              if (event.key === "Enter") commitRename();
              if (event.key === "Escape") {
                setDraft(timer.label);
                setEditing(false);
              }
            }}
            className="w-full min-w-0 rounded-md border border-accent/30 bg-inset px-2 py-1 text-center text-[13px] text-fg-strong outline-none"
          />
        ) : (
          <div className="group flex min-w-0 max-w-full items-center justify-center gap-1">
            <Tooltip content="Double-click to rename">
              <button
                type="button"
                onDoubleClick={() => {
                  setDraft(timer.label);
                  setEditing(true);
                }}
                className="min-w-0 truncate text-center text-[13px] font-semibold text-fg-strong hover:text-accent transition-colors"
              >
                {timer.label}
              </button>
            </Tooltip>
          </div>
        )}
      </div>

      <HourRing
        elapsed={elapsed}
        running={running}
        onToggle={() => (running ? onPause(timer.id) : onStart(timer.id))}
      />

      <div
        className={`font-mono text-[26px] font-semibold tabular-nums leading-none tracking-tight ${
          running ? "text-accent" : "text-fg-soft"
        }`}
      >
        {formatDuration(elapsed)}
      </div>

      {dialog === "time" && (
        <TimerSegmentsSheet
          timer={timer}
          total={elapsed}
          runningMs={elapsed - timer.accumulatedMs}
          onAdd={(durationMs) => onAddSegment(timer.id, durationMs)}
          onUpdate={(segmentId, durationMs) =>
            onUpdateSegment(timer.id, segmentId, durationMs)
          }
          onDelete={(segmentId) => onDeleteSegment(timer.id, segmentId)}
          onClose={() => setDialog(null)}
        />
      )}

      {dialog === "saved" && (
        <TimerSavedStatesSheet timer={timer} onClose={() => setDialog(null)} />
      )}

      {dialog === "calendar" && calendarRange !== null && (
        <AddTimeSheet
          date={new Date(calendarRange.start)}
          initialStartMs={calendarRange.start}
          initialEndMs={calendarRange.end}
          initialDescription={timer.label}
          categories={categories}
          projects={projects}
          onClose={() => setDialog(null)}
          onCreated={() => onClear(timer.id)}
        />
      )}

      {pending === "reset" && (
        <ConfirmDialog
          title={`Reset "${timer.label}"?`}
          body="Set its time back to 0:00 and stop it."
          confirmLabel="Reset"
          onConfirm={() => {
            setPending(null);
            onReset(timer.id);
          }}
          onCancel={() => setPending(null)}
        />
      )}

      {pending === "delete" && (
        <ConfirmDialog
          title={`Delete "${timer.label}"?`}
          body={`This tracker, its ${formatDuration(elapsed)} and its saved states will be gone.`}
          confirmLabel="Delete"
          onConfirm={() => {
            setPending(null);
            onDelete(timer.id);
          }}
          onCancel={() => setPending(null)}
        />
      )}
    </section>
  );
}

interface TimerMenuProps {
  label: string;
  canAddToCalendar: boolean;
  onSaveState: () => void;
  onShowSavedStates: () => void;
  onRename: () => void;
  onEditTime: () => void;
  onAddToCalendar: () => void;
  onReset: () => void;
  onDelete: () => void;
}

/** The card's three-dot menu, pinned to its top right corner. */
function TimerMenu({
  label,
  canAddToCalendar,
  onSaveState,
  onShowSavedStates,
  onRename,
  onEditTime,
  onAddToCalendar,
  onReset,
  onDelete,
}: TimerMenuProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, ref, close);

  const item = (
    text: string,
    action: () => void,
    options: { danger?: boolean; disabled?: boolean; hint?: string } = {},
  ) => (
    <button
      type="button"
      role="menuitem"
      disabled={options.disabled}
      title={options.disabled ? options.hint : undefined}
      onClick={() => {
        setOpen(false);
        action();
      }}
      className={`flex w-full items-center rounded-md px-2.5 py-1.5 text-left text-[12px] disabled:cursor-not-allowed disabled:opacity-40 ${
        options.danger
          ? "text-danger hover:bg-danger-soft"
          : "text-fg-muted hover:bg-surface hover:text-fg disabled:hover:bg-transparent"
      }`}
    >
      {text}
    </button>
  );

  return (
    <div ref={ref} className="absolute top-2 right-2">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`Options for ${label}`}
        onClick={() => setOpen(!open)}
        className="grid size-7 place-items-center rounded-md text-fg-faint transition-colors hover:bg-surface hover:text-fg"
      >
        <svg
          viewBox="0 0 24 24"
          className="size-4"
          fill="currentColor"
          aria-hidden="true"
        >
          <circle cx="12" cy="5" r="1.7" />
          <circle cx="12" cy="12" r="1.7" />
          <circle cx="12" cy="19" r="1.7" />
        </svg>
      </button>
      {open && (
        <div
          role="menu"
          className="absolute top-full right-0 z-30 mt-1 w-48 rounded-lg border border-line bg-panel p-1 shadow-black/40 shadow-xl"
        >
          {item("Save state", onSaveState)}
          {item("Saved states...", onShowSavedStates)}
          <hr className="my-1 border-line" />
          {item("Rename", onRename)}
          {item("Edit time...", onEditTime)}
          {item("Add to calendar...", onAddToCalendar, {
            disabled: !canAddToCalendar,
            hint: "Needs at least a minute of tracked time",
          })}
          <hr className="my-1 border-line" />
          {item("Reset to zero", onReset)}
          {item("Delete", onDelete, { danger: true })}
        </div>
      )}
    </div>
  );
}
