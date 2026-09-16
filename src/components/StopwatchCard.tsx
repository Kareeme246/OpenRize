import { useState } from "react";
import { ConfirmDialog } from "./ConfirmDialog";
import { HourRing } from "./HourRing";
import { elapsedMs, formatDuration, type Timer } from "../lib/timers";

interface StopwatchCardProps {
  timer: Timer;
  now: number;
  onStart: (id: string) => void;
  onPause: (id: string) => void;
  onReset: (id: string) => void;
  onRename: (id: string, label: string) => void;
  onDelete: (id: string) => void;
}

export function StopwatchCard({
  timer,
  now,
  onStart,
  onPause,
  onReset,
  onRename,
  onDelete,
}: StopwatchCardProps) {
  const running = timer.startedAt !== null;
  const elapsed = elapsedMs(timer, now);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(timer.label);
  /** Which confirmation is up, if any. Both destroy real accumulated time. */
  const [pending, setPending] = useState<"reset" | "delete" | null>(null);

  const commitRename = (): void => {
    const next = draft.trim();
    if (next.length > 0 && next !== timer.label) onRename(timer.id, next);
    else setDraft(timer.label);
    setEditing(false);
  };

  return (
    <div
      className={`flex min-w-0 flex-col gap-3 rounded-xl border p-4 ${
        running
          ? "border-accent/30 bg-linear-to-b from-accent/10 to-accent/2"
          : "border-white/10 bg-linear-to-b from-white/5 to-white/1"
      }`}
    >
      <div className="flex min-w-0 items-center justify-between gap-2.5">
        {editing ? (
          <input
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
            className="min-w-0 flex-1 rounded-md border border-accent/30 bg-black/50 px-2 py-1 text-[13px] text-white outline-none"
          />
        ) : (
          <button
            type="button"
            title="Double-click to rename"
            onDoubleClick={() => {
              setDraft(timer.label);
              setEditing(true);
            }}
            className="min-w-0 flex-1 truncate text-left text-[13px] font-semibold"
          >
            {timer.label}
          </button>
        )}
      </div>

      <div className="flex min-w-0 items-center gap-3.5">
        <HourRing elapsed={elapsed} running={running} />
        <div className="min-w-0">
          <div
            className={`truncate font-mono text-[26px] font-semibold tabular-nums leading-none tracking-tight ${
              running
                ? "text-accent drop-shadow-[0_0_28px_rgba(42,234,131,0.35)]"
                : "text-white/45"
            }`}
          >
            {formatDuration(elapsed)}
          </div>
          <div
            className={`mt-1.5 font-mono text-[9.5px] uppercase tracking-wider ${
              running ? "text-accent/70" : "text-white/30"
            }`}
          >
            {running ? "running" : "paused"}
          </div>
        </div>
      </div>

      <div className="flex gap-1.5">
        <button
          type="button"
          onClick={() => (running ? onPause(timer.id) : onStart(timer.id))}
          className={`flex-1 rounded-lg border py-1.5 text-xs font-semibold ${
            running
              ? "border-accent/30 bg-accent-soft text-accent"
              : "border-white/10 bg-white/5 text-white"
          }`}
        >
          {running ? "Pause" : "Start"}
        </button>
        <button
          type="button"
          title="Reset to zero and stop"
          aria-label="Reset to zero and stop"
          onClick={() => setPending("reset")}
          className="grid w-9 shrink-0 place-items-center rounded-lg border border-white/10 bg-white/5 text-white/55"
        >
          <svg
            viewBox="0 0 24 24"
            className="size-3.5"
            fill="none"
            stroke="currentColor"
            strokeWidth={2.2}
            strokeLinecap="round"
            aria-hidden="true"
          >
            <path d="M3 11a9 9 0 1 0 3-6.7M3 4v5h5" />
          </svg>
        </button>
        <button
          type="button"
          title="Delete"
          aria-label="Delete"
          onClick={() => setPending("delete")}
          className="grid w-9 shrink-0 place-items-center rounded-lg border border-white/10 bg-white/5 text-sm text-white/55"
        >
          ×
        </button>
      </div>

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
          body={`This tracker and its ${formatDuration(elapsed)} will be gone.`}
          confirmLabel="Delete"
          danger
          onConfirm={() => {
            setPending(null);
            onDelete(timer.id);
          }}
          onCancel={() => setPending(null)}
        />
      )}
    </div>
  );
}
