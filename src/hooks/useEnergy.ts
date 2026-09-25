import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/api";
import type { EnergySummary } from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

export interface EnergyApi {
  summary: EnergySummary | null;
  error: string | null;
  loading: boolean;
  refresh: () => void;
  resetHistory: () => Promise<void>;
  adopt: (summary: EnergySummary) => void;
}

/**
 * Energy usage and battery monitor telemetry hook.
 *
 * Refetches on interval or whenever Rust emits `energy-changed`.
 */
export function useEnergy(days = 7): EnergyApi {
  const [summary, setSummary] = useState<EnergySummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState<boolean>(true);

  const fetchSummary = useCallback(() => {
    let active = true;
    api
      .getEnergySummary(days)
      .then((loaded) => {
        if (!active) return;
        setSummary(loaded);
        setError(null);
        setLoading(false);
      })
      .catch((cause: unknown) => {
        if (!active) return;
        setError(api.describeError(cause));
        setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [days]);

  useEffect(() => {
    setLoading(true);
    const cancel = fetchSummary();
    return cancel;
  }, [fetchSummary]);

  useTauriEvent<EnergySummary>(api.ENERGY_CHANGED, (next) => {
    // If emitted summary matches the requested days window, adopt directly;
    // otherwise refetch for the selected days.
    if (next.windowDays === days) {
      setSummary(next);
      setError(null);
    } else {
      fetchSummary();
    }
  });

  const adopt = useCallback((next: EnergySummary): void => {
    setSummary(next);
    setError(null);
  }, []);

  const resetHistory = useCallback(async (): Promise<void> => {
    try {
      const next = await api.resetEnergyHistory();
      setSummary(next);
      setError(null);
    } catch (cause: unknown) {
      setError(api.describeError(cause));
      throw cause;
    }
  }, []);

  return {
    summary,
    error,
    loading,
    refresh: fetchSummary,
    resetHistory,
    adopt,
  };
}
