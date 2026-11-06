import { useEffect, useState } from "react";
import * as api from "../lib/api";
import { type BreakState, IDLE_BREAK_STATE } from "../lib/breaks";
import { useTauriEvent } from "./useTauriEvent";

export interface BreaksApi {
  state: BreakState;
  now: number;
}

/**
 * Rust's break state from `break-state-changed`; countdowns derive from its
 * timestamps against `now`, so events fire only on structural changes.
 */
export function useBreaks(): BreaksApi {
  const [state, setState] = useState<BreakState>(IDLE_BREAK_STATE);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    api.breakState().then(setState, () => undefined);
  }, []);
  useTauriEvent<BreakState>(api.BREAK_STATE_CHANGED, setState);

  const ticking =
    state.phase !== "idle" || state.next !== null || state.stopwatch !== null;
  useEffect(() => {
    if (!ticking) return;
    setNow(Date.now());
    const handle = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(handle);
  }, [ticking]);

  return { state, now };
}
