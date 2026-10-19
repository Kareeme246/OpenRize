import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import {
  formatDuration,
  type Timer,
  type TimerRestore,
  type TimerSavedState,
} from "../lib/timers";
import { useInverseToast } from "./useChangeToast";
import { useTauriEvent } from "./useTauriEvent";

export interface TimersApi {
  timers: Timer[];
  /** Shared 1 Hz clock; every card reads this same value. */
  now: number;
  error: string | null;
  loading: boolean;
  create: (label: string) => void;
  start: (id: string) => void;
  pause: (id: string) => void;
  reset: (id: string) => void;
  /** Resets without a toast, once the time has been logged elsewhere. */
  clear: (id: string) => void;
  rename: (id: string, label: string) => void;
  addSegment: (id: string, durationMs: number) => void;
  updateSegment: (id: string, segmentId: number, durationMs: number) => void;
  deleteSegment: (id: string, segmentId: number) => void;
  saveState: (id: string) => void;
  remove: (id: string) => void;
}

let cachedTimers: Timer[] | null = null;

/** The rows an undo puts back: `timer` as it was, plus its saved states. */
function restoreOf(
  timer: Timer,
  savedStates: TimerSavedState[] = [],
): TimerRestore {
  return {
    id: timer.id,
    label: timer.label,
    startedAt: timer.startedAt,
    createdAt: timer.createdAt,
    segments: timer.segments,
    savedStates,
  };
}

export function useTimers(): TimersApi {
  const [timers, setTimersState] = useState<Timer[]>(() => cachedTimers ?? []);
  const [loading, setLoading] = useState<boolean>(cachedTimers === null);
  const [now, setNow] = useState<number>(() => Date.now());
  const [error, setError] = useState<string | null>(null);
  const timersRef = useRef(timers);
  timersRef.current = timers;

  const setTimers = useCallback((next: Timer[]) => {
    cachedTimers = next;
    setTimersState(next);
    setLoading(false);
  }, []);

  // Adopt whatever Rust already has on disk. Rust owns the state, so there is
  // no default array to render in the meantime.
  useEffect(() => {
    api
      .listTimers()
      .then(setTimers)
      .catch((cause: unknown) => {
        setError(describeError(cause));
        setLoading(false);
      });
  }, [setTimers]);

  // ONE interval for the whole page. N running stopwatches cost one repaint per
  // second, not N — this is the reason the tick lives here and not in a card.
  useEffect(() => {
    const handle = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(handle);
  }, []);

  // The tray can pause a timer without the frontend asking, so Rust broadcasts
  // the authoritative list and we replace state from it.
  useTauriEvent<Timer[]>(api.TIMERS_CHANGED, (payload) => {
    setTimers(payload);
    setError(null);
  });

  // Every command answers with the full list, so state is replaced, never
  // patched — the UI cannot invent a timer that is not on disk. Resolves to
  // that list, or null when the command failed.
  const send = useCallback(
    (action: Promise<Timer[]>): Promise<Timer[] | null> =>
      action
        .then((next) => {
          setTimers(next);
          return next;
        })
        .catch((cause: unknown) => {
          setError(describeError(cause));
          return null;
        }),
    [setTimers],
  );

  // An undo answers with the full list too; adopt it like any command's.
  const confirmChange = useInverseToast();
  const confirm = useCallback(
    (
      message: string,
      undo?: { run: () => Promise<Timer[] | undefined>; undone: string },
    ): void =>
      confirmChange(
        message,
        undo && {
          run: () =>
            undo.run().then((next) => {
              if (next) setTimers(next);
            }),
          undone: undo.undone,
        },
      ),
    [confirmChange, setTimers],
  );

  const find = useCallback(
    (id: string): Timer | undefined =>
      timersRef.current.find((timer) => timer.id === id),
    [],
  );

  const create = useCallback(
    (label: string) => {
      const known = new Set(timersRef.current.map((timer) => timer.id));
      void send(api.createTimer(label)).then((next) => {
        const created = next?.find((timer) => !known.has(timer.id));
        if (!created) return;
        confirm(`Created "${created.label}"`, {
          // Only an untouched tracker goes: one that has counted time stays.
          run: () => {
            const current = find(created.id);
            if (
              current &&
              (current.startedAt !== null || current.segments.length > 0)
            ) {
              return Promise.reject(
                new Error("it has counted time since, so it stays"),
              );
            }
            return api.deleteTimer(created.id);
          },
          undone: `Removed "${created.label}"`,
        });
      });
    },
    [send, confirm, find],
  );
  // Starting and pausing show on the card itself; a toast would only repeat it.
  const start = useCallback(
    (id: string) => void send(api.startTimer(id)),
    [send],
  );
  const pause = useCallback(
    (id: string) => void send(api.pauseTimer(id)),
    [send],
  );
  const reset = useCallback(
    (id: string) => {
      const before = find(id);
      void send(api.resetTimer(id)).then((next) => {
        if (!next || !before) return;
        confirm(`Reset "${before.label}"`, {
          run: () => api.restoreTimer(restoreOf(before)),
          undone: `Restored "${before.label}"`,
        });
      });
    },
    [send, confirm, find],
  );
  const clear = useCallback(
    (id: string) => void send(api.resetTimer(id)),
    [send],
  );
  const rename = useCallback(
    (id: string, label: string) => {
      const previous = find(id)?.label;
      void send(api.renameTimer(id, label)).then((next) => {
        if (!next || previous === undefined) return;
        confirm(`Renamed to "${label}"`, {
          run: () => api.renameTimer(id, previous),
          undone: `Renamed back to "${previous}"`,
        });
      });
    },
    [send, confirm, find],
  );
  const addSegment = useCallback(
    (id: string, durationMs: number) => {
      const known = new Set(find(id)?.segments.map((segment) => segment.id));
      void send(api.addTimerSegment(id, durationMs)).then((next) => {
        const timer = next?.find((candidate) => candidate.id === id);
        const added = timer?.segments.find((segment) => !known.has(segment.id));
        if (!timer || !added) return;
        confirm(`Added ${formatDuration(durationMs)} to "${timer.label}"`, {
          run: () => api.deleteTimerSegment(id, added.id),
          undone: `Removed the added ${formatDuration(durationMs)}`,
        });
      });
    },
    [send, confirm, find],
  );
  const updateSegment = useCallback(
    (id: string, segmentId: number, durationMs: number) => {
      const timer = find(id);
      const previous = timer?.segments.find(
        (segment) => segment.id === segmentId,
      );
      void send(api.updateTimerSegment(id, segmentId, durationMs)).then(
        (next) => {
          if (!next || !timer || !previous) return;
          confirm(
            `Changed a segment of "${timer.label}" to ${formatDuration(durationMs)}`,
            {
              run: () =>
                api.updateTimerSegment(id, segmentId, previous.durationMs),
              undone: `Changed it back to ${formatDuration(previous.durationMs)}`,
            },
          );
        },
      );
    },
    [send, confirm, find],
  );
  const deleteSegment = useCallback(
    (id: string, segmentId: number) => {
      const timer = find(id);
      const removed = timer?.segments.find(
        (segment) => segment.id === segmentId,
      );
      void send(api.deleteTimerSegment(id, segmentId)).then((next) => {
        if (!next || !timer || !removed) return;
        confirm(
          `Removed ${formatDuration(removed.durationMs)} from "${timer.label}"`,
          {
            // Only the segment: the run, if any, was never touched.
            run: () =>
              api.restoreTimer({
                ...restoreOf(timer),
                startedAt: null,
                segments: [removed],
              }),
            undone: `Put back ${formatDuration(removed.durationMs)}`,
          },
        );
      });
    },
    [send, confirm, find],
  );
  // A save changes nothing about the timers, so there is no list to adopt;
  // it is read back through the Saved states sheet.
  const saveState = useCallback(
    (id: string) => {
      api
        .saveTimerState(id)
        .then((saved) =>
          confirm(
            `Saved "${saved.label}" at ${formatDuration(saved.elapsedMs)}`,
            {
              run: () =>
                api.deleteTimerSavedState(saved.id).then(() => undefined),
              undone: "Removed the saved state",
            },
          ),
        )
        .catch((cause: unknown) => setError(describeError(cause)));
    },
    [confirm],
  );
  const remove = useCallback(
    (id: string) => {
      const before = find(id);
      void (async () => {
        // The card does not hold its saved states; without them an undo would
        // bring the tracker back short, so then it offers none.
        const savedStates = await api
          .listTimerSavedStates(id)
          .catch(() => null);
        const next = await send(api.deleteTimer(id));
        if (!next || !before) return;
        confirm(
          `Deleted "${before.label}"`,
          savedStates === null
            ? undefined
            : {
                run: () => api.restoreTimer(restoreOf(before, savedStates)),
                undone: `Restored "${before.label}"`,
              },
        );
      })();
    },
    [send, confirm, find],
  );

  return {
    timers,
    now,
    error,
    loading,
    create,
    start,
    pause,
    reset,
    clear,
    rename,
    addSegment,
    updateSegment,
    deleteSegment,
    saveState,
    remove,
  };
}
