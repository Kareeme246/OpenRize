import { useEffect, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import {
  formatDuration,
  type Timer,
  type TimerSavedState,
} from "../lib/timers";
import { Sheet } from "./Sheet";

interface TimerSavedStatesSheetProps {
  timer: Timer;
  onClose: () => void;
}

/** Read-only history of the moments the user chose to record, newest first. */
export function TimerSavedStatesSheet({
  timer,
  onClose,
}: TimerSavedStatesSheetProps) {
  const [states, setStates] = useState<TimerSavedState[] | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  return (
    <Sheet
      title={`Saved states - ${timer.label}`}
      submitLabel="Done"
      onSubmit={onClose}
      onClose={onClose}
      error={error}
      width="sm"
    >
      <div className="font-medium text-[11.5px] text-fg-soft">
        Saved readings
      </div>
      {states === null ? (
        error === null && <p className="text-fg-faint">Loading...</p>
      ) : states.length === 0 ? (
        <p className="text-fg-faint">
          No saved states yet. Choose Save state in the menu to record the time
          as it reads now.
        </p>
      ) : (
        <ul className="space-y-2">
          {states.map((state) => (
            <li
              key={state.id}
              className="flex items-center justify-between gap-2"
            >
              <span className="min-w-0 truncate text-fg-muted">
                {new Date(state.savedAt).toLocaleString([], {
                  month: "short",
                  day: "numeric",
                  hour: "numeric",
                  minute: "2-digit",
                })}
              </span>
              <span className="font-mono text-fg-strong tabular-nums">
                {formatDuration(state.elapsedMs)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </Sheet>
  );
}
