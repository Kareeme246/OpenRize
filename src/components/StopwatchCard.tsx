import { useEffect, useState } from "react";
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

/** Deleting a stopwatch throws away real accumulated time, so it takes two
 *  clicks. A native confirm() is one line but its behaviour differs by webview,
 *  and a modal is more surface than this deserves. */
const CONFIRM_WINDOW_MS = 3000;

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
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(timer.label);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    if (!confirming) return;
    const handle = window.setTimeout(
      () => setConfirming(false),
      CONFIRM_WINDOW_MS,
    );
    return () => window.clearTimeout(handle);
  }, [confirming]);

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
        <HourRing now={now} running={running} />
        <div className="min-w-0">
          <div
            className={`truncate font-mono text-[26px] font-semibold tabular-nums leading-none tracking-tight ${
              running
                ? "text-accent drop-shadow-[0_0_28px_rgba(42,234,131,0.35)]"
                : "text-white/45"
            }`}
          >
            {formatDuration(elapsedMs(timer, now))}
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
          title="Reset to zero"
          aria-label="Reset to zero"
          onClick={() => onReset(timer.id)}
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
          title={confirming ? "Click again to delete" : "Delete"}
          aria-label={confirming ? "Confirm delete" : "Delete"}
          onClick={() =>
            confirming ? onDelete(timer.id) : setConfirming(true)
          }
          className={`grid w-9 shrink-0 place-items-center rounded-lg border text-sm ${
            confirming
              ? "border-red-400/50 bg-red-400/10 text-red-300"
              : "border-white/10 bg-white/5 text-white/55"
          }`}
        >
          {confirming ? "!" : "×"}
        </button>
      </div>
    </div>
  );
}
