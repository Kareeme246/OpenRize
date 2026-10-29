import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { formatDuration, type SegmentEdit, type Timer } from "../lib/timers";
import { useTauriEvent } from "./useTauriEvent";
import { useToast } from "./useToast";

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
  addSegment: (id: string, startedAt: number, endedAt: number) => void;
  updateSegment: (id: string, segmentId: number, edit: SegmentEdit) => void;
  deleteSegment: (id: string, segmentId: number) => void;
  saveState: (id: string) => void;
  remove: (id: string) => void;
  reorder: (ids: string[]) => void;
  moveTimer: (id: string, direction: "up" | "down") => void;
}

let cachedTimers: Timer[] | null = null;

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

  const showToast = useToast();
  const confirm = useCallback(
    (action: Promise<Timer[]>, message: string): void => {
      void send(action).then((next) => {
        if (next) showToast({ message, tone: "success" });
      });
    },
    [send, showToast],
  );
  const labelOf = useCallback(
    (id: string): string =>
      timersRef.current.find((timer) => timer.id === id)?.label ?? "Timer",
    [],
  );
  const create = useCallback(
    (label: string) => confirm(api.createTimer(label), `Created "${label}"`),
    [confirm],
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
    (id: string) => confirm(api.resetTimer(id), `Reset "${labelOf(id)}"`),
    [confirm, labelOf],
  );
  const clear = useCallback(
    (id: string) => void send(api.resetTimer(id)),
    [send],
  );
  const rename = useCallback(
    (id: string, label: string) =>
      confirm(api.renameTimer(id, label), `Renamed to "${label}"`),
    [confirm],
  );
  const addSegment = useCallback(
    (id: string, startedAt: number, endedAt: number) =>
      confirm(
        api.addTimerSegment(id, startedAt, endedAt),
        `Added ${formatDuration(endedAt - startedAt)} to "${labelOf(id)}"`,
      ),
    [confirm, labelOf],
  );
  const updateSegment = useCallback(
    (id: string, segmentId: number, edit: SegmentEdit) =>
      confirm(
        api.updateTimerSegment(id, segmentId, edit),
        `Segment changed on "${labelOf(id)}"`,
      ),
    [confirm, labelOf],
  );
  const deleteSegment = useCallback(
    (id: string, segmentId: number) =>
      confirm(
        api.deleteTimerSegment(id, segmentId),
        `Segment removed from "${labelOf(id)}"`,
      ),
    [confirm, labelOf],
  );
  // A save changes nothing about the timers, so there is no list to adopt.
  const saveState = useCallback(
    (id: string) => {
      api
        .saveTimerState(id)
        .then((saved) =>
          showToast({
            message: `Saved "${saved.label}" at ${formatDuration(saved.elapsedMs)}`,
            tone: "success",
          }),
        )
        .catch((cause: unknown) => setError(describeError(cause)));
    },
    [showToast],
  );
  const remove = useCallback(
    (id: string) => confirm(api.deleteTimer(id), `Deleted "${labelOf(id)}"`),
    [confirm, labelOf],
  );
  const reorder = useCallback(
    (ids: string[]) => send(api.reorderTimers(ids)),
    [send],
  );
  const moveTimer = useCallback(
    (id: string, direction: "up" | "down") =>
      send(api.moveTimer(id, direction === "up" ? -1 : 1)),
    [send],
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
    reorder,
    moveTimer,
  };
}
