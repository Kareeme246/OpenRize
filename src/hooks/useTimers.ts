import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import type { Timer } from "../lib/timers";

export interface TimersApi {
  timers: Timer[];
  /** Shared 1 Hz clock; every card reads this same value. */
  now: number;
  error: string | null;
  create: (label: string) => void;
  start: (id: string) => void;
  pause: (id: string) => void;
  reset: (id: string) => void;
  rename: (id: string, label: string) => void;
  remove: (id: string) => void;
}

export function useTimers(): TimersApi {
  const [timers, setTimers] = useState<Timer[]>([]);
  const [now, setNow] = useState<number>(() => Date.now());
  const [error, setError] = useState<string | null>(null);

  // Adopt whatever Rust already has on disk. Rust owns the state, so there is
  // no default array to render in the meantime.
  useEffect(() => {
    api.listTimers().then(setTimers).catch((cause: unknown) => setError(describeError(cause)));
  }, []);

  // ONE interval for the whole page. N running stopwatches cost one repaint per
  // second, not N — this is the reason the tick lives here and not in a card.
  useEffect(() => {
    const handle = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(handle);
  }, []);

  // The tray can pause a timer without the frontend asking, so Rust broadcasts
  // the authoritative list and we replace state from it.
  useEffect(() => {
    const pending = listen<Timer[]>(api.TIMERS_CHANGED, (event) => {
      setTimers(event.payload);
      setError(null);
    });
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, []);

  // Every command answers with the full list, so state is replaced, never
  // patched — the UI cannot invent a timer that is not on disk.
  const send = useCallback((action: Promise<Timer[]>): void => {
    action.then(setTimers).catch((cause: unknown) => setError(describeError(cause)));
  }, []);

  const create = useCallback((label: string) => send(api.createTimer(label)), [send]);
  const start = useCallback((id: string) => send(api.startTimer(id)), [send]);
  const pause = useCallback((id: string) => send(api.pauseTimer(id)), [send]);
  const reset = useCallback((id: string) => send(api.resetTimer(id)), [send]);
  const rename = useCallback((id: string, label: string) => send(api.renameTimer(id, label)), [send]);
  const remove = useCallback((id: string) => send(api.deleteTimer(id)), [send]);

  return { timers, now, error, create, start, pause, reset, rename, remove };
}
