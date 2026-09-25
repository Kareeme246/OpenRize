import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/api";
import type { AiMetrics } from "../lib/types";

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

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const load = (): void => {
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
    };
    const schedule = (): void => {
      clearTimeout(timer);
      timer = setTimeout(load, REFETCH_DEBOUNCE_MS);
    };
    load();
    const stops = [
      listen(api.ENTRIES_CHANGED, schedule),
      listen(api.AI_STATUS_CHANGED, schedule),
    ];
    return () => {
      active = false;
      clearTimeout(timer);
      for (const stop of stops) void stop.then((unlisten) => unlisten());
    };
  }, [days]);

  const adopt = useCallback((next: AiMetrics): void => {
    setMetrics(next);
    setError(null);
  }, []);

  return { metrics, error, adopt };
}
