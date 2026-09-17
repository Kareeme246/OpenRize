import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  type ActivitySnapshot,
  type ActivityTick,
  type SessionKind,
  startOfToday,
} from "../lib/activity";
import * as api from "../lib/api";
import { describeError } from "../lib/api";

export interface ActivityApi {
  /** Null until the first load lands; Rust owns the state either way. */
  snapshot: ActivitySnapshot | null;
  /** Shared 1 Hz clock for the live session timer. */
  now: number;
  error: string | null;
  setCaptureEnabled: (enabled: boolean) => void;
  setIdleThreshold: (minutes: number) => void;
  startSession: (kind: SessionKind, label?: string) => void;
  stopSession: () => void;
  markReviewed: (id: number) => void;
}

/**
 * Reads the automatic-capture snapshot and mirrors Rust's mutations back.
 * The day bound is computed here (local midnight) and passed down, because
 * Rust has no timezone information.
 */
export function useActivity(): ActivityApi {
  const [snapshot, setSnapshot] = useState<ActivitySnapshot | null>(null);
  const [now, setNow] = useState<number>(() => Date.now());
  const [error, setError] = useState<string | null>(null);
  // Guards against a slow response overwriting a newer one.
  const requestId = useRef(0);

  const refresh = useCallback((): void => {
    const id = ++requestId.current;
    api
      .activitySnapshot(startOfToday())
      .then((next) => {
        if (id !== requestId.current) return;
        setSnapshot(next);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (id !== requestId.current) return;
        setError(describeError(cause));
      });
  }, []);

  useEffect(refresh, [refresh]);

  // One interval for the page, same reasoning as useTimers: the live session
  // timer must tick without every card owning a clock. The sampler emits its
  // own event; this only exists to keep the elapsed readout moving.
  useEffect(() => {
    const handle = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(handle);
  }, []);

  // A structural change (segment opened/closed) or a focus-regained
  // reconciliation carries the full snapshot directly — adopt it, same
  // pattern useTimers already uses for timers-changed. No follow-up call.
  useEffect(() => {
    const pending = listen<ActivitySnapshot>(api.ACTIVITY_CHANGED, (event) => {
      setSnapshot(event.payload);
      setError(null);
    });
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, []);

  // The lightweight push (1Hz while OpenRize is focused, a 30s heartbeat
  // otherwise) carries the same numbers minus the segment list — merge them
  // in without touching `segments`, which only ever changes on the event
  // above.
  useEffect(() => {
    const pending = listen<ActivityTick>(api.ACTIVITY_TICK, (event) => {
      setSnapshot((previous) =>
        previous === null ? previous : { ...previous, ...event.payload },
      );
    });
    return () => {
      void pending.then((unlisten) => unlisten());
    };
  }, []);

  // Mutations are fire-and-forget: Rust answers with an event that triggers
  // refresh above, so there is exactly one path that writes state.
  const run = useCallback((action: Promise<void>): void => {
    action.catch((cause: unknown) => setError(describeError(cause)));
  }, []);

  const setCaptureEnabled = useCallback(
    (enabled: boolean) => run(api.setCaptureEnabled(enabled)),
    [run],
  );
  const setIdleThreshold = useCallback(
    (minutes: number) => run(api.setIdleThreshold(minutes)),
    [run],
  );
  const startSession = useCallback(
    (kind: SessionKind, label?: string) => run(api.startSession(kind, label)),
    [run],
  );
  const stopSession = useCallback(() => run(api.stopSession()), [run]);
  const markReviewed = useCallback(
    (id: number) => run(api.markSegmentReviewed(id)),
    [run],
  );

  return {
    snapshot,
    now,
    error,
    setCaptureEnabled,
    setIdleThreshold,
    startSession,
    stopSession,
    markReviewed,
  };
}
