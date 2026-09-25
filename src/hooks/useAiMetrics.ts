import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../lib/api";
import type { AiMetrics } from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

/** Coalesces a burst of entry and status events into one refetch. */
const REFETCH_DEBOUNCE_MS = 400;

export interface AiMetricsApi {
  metrics: AiMetrics | null;
  error: string | null;
  /** Adopts a snapshot a mutating command already returned. */
  adopt: (metrics: AiMetrics) => void;
}

/**
 * The AI effectiveness snapshot for the last `days` days, refetched when
 * entries change (a review lands) or the engine status changes (a retrain
 * or calibration refit finished).
 */
export function useAiMetrics(days: number): AiMetricsApi {
  const [metrics, setMetrics] = useState<AiMetrics | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Bumped (debounced) by the events below; each bump refetches.
  const [version, setVersion] = useState(0);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` is the refetch trigger.
  useEffect(() => {
    let active = true;
    api
      .aiMetrics(days)
      .then((loaded) => {
        if (!active) return;
        setMetrics(loaded);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (active) setError(api.describeError(cause));
      });
    return () => {
      active = false;
    };
  }, [days, version]);

  const schedule = (): void => {
    clearTimeout(timer.current);
    timer.current = setTimeout(
      () => setVersion((current) => current + 1),
      REFETCH_DEBOUNCE_MS,
    );
  };
  useEffect(() => () => clearTimeout(timer.current), []);
  useTauriEvent(api.ENTRIES_CHANGED, schedule);
  useTauriEvent(api.AI_STATUS_CHANGED, schedule);

  const adopt = useCallback((next: AiMetrics): void => {
    setMetrics(next);
    setError(null);
  }, []);

  return { metrics, error, adopt };
}
