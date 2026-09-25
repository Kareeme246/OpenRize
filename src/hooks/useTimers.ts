import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import type { Timer } from "../lib/timers";
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
  rename: (id: string, label: string) => void;
  remove: (id: string) => void;
}

let cachedTimers: Timer[] | null = null;

export function useTimers(): TimersApi {
  const [timers, setTimersState] = useState<Timer[]>(() => cachedTimers ?? []);
  const [loading, setLoading] = useState<boolean>(cachedTimers === null);
  const [now, setNow] = useState<number>(() => Date.now());
  const [error, setError] = useState<string | null>(null);

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
  // patched — the UI cannot invent a timer that is not on disk.
  const send = useCallback(
    (action: Promise<Timer[]>): void => {
      action
        .then(setTimers)
        .catch((cause: unknown) => setError(describeError(cause)));
    },
    [setTimers],
  );

  const create = useCallback(
    (label: string) => send(api.createTimer(label)),
    [send],
  );
  const start = useCallback((id: string) => send(api.startTimer(id)), [send]);
  const pause = useCallback((id: string) => send(api.pauseTimer(id)), [send]);
  const reset = useCallback((id: string) => send(api.resetTimer(id)), [send]);
  const rename = useCallback(
    (id: string, label: string) => send(api.renameTimer(id, label)),
    [send],
  );
  const remove = useCallback((id: string) => send(api.deleteTimer(id)), [send]);

  return {
    timers,
    now,
    error,
    loading,
    create,
    start,
    pause,
    reset,
    rename,
    remove,
  };
}
