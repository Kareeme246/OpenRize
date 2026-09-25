import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { splitPoint } from "../lib/entries";
import type {
  EntryDetail,
  RuleSuggestion,
  SuggestionField,
} from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

/**
 * The one entry review panel's state and commands, shared by every view that
 * opens it (Calendar, My Timesheet, Time Entries, a project's recent
 * entries). Each command goes to Rust and re-reads the panel; the views'
 * lists refresh from the `entries-changed` event those commands emit.
 */
export interface EntryReview {
  selectedId?: string;
  detail: EntryDetail | null;
  error: string | null;
  select: (id?: string) => void;
  /** Resolves true once approved, false when it failed. */
  accept: (id: string) => Promise<boolean>;
  reject: (id: string) => Promise<boolean>;
  setField: (field: SuggestionField, valueId: string | null) => Promise<void>;
  toggleBillable: () => Promise<void>;
  saveDescription: (description: string) => Promise<void>;
  /** Splits the open entry and opens its first half. */
  split: () => Promise<void>;
  remove: () => Promise<void>;
  retry: () => Promise<void>;
  resolveRule: (suggestion: RuleSuggestion, accept: boolean) => Promise<void>;
}

export function useEntryReview(initialId?: string): EntryReview {
  const [selectedId, setSelectedId] = useState<string | undefined>(initialId);
  const [detail, setDetail] = useState<EntryDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const selectedRef = useRef(selectedId);
  selectedRef.current = selectedId;

  const load = useCallback(async (id: string | undefined): Promise<void> => {
    if (!id) {
      setDetail(null);
      return;
    }
    try {
      const next = await api.getEntryDetail(id);
      // A slower read for an entry the user already left must not win.
      if (selectedRef.current === id) setDetail(next);
    } catch (cause) {
      if (selectedRef.current === id) {
        setDetail(null);
        setError(describeError(cause));
      }
    }
  }, []);

  useEffect(() => {
    setError(null);
    load(selectedId);
  }, [selectedId, load]);

  // Suggestions land in the background: patch the open panel.
  useTauriEvent(api.ENTRIES_CHANGED, () => void load(selectedRef.current));
  useTauriEvent<{ entryId: string }>(api.SUGGESTION_READY, (payload) => {
    if (payload.entryId === selectedRef.current) void load(selectedRef.current);
  });

  const select = useCallback((id?: string) => setSelectedId(id), []);

  /** Runs a command, surfacing its error, then re-reads the panel. */
  const run = useCallback(
    async (action: () => Promise<unknown>): Promise<boolean> => {
      try {
        await action();
        setError(null);
        await load(selectedRef.current);
        return true;
      } catch (cause) {
        setError(describeError(cause));
        return false;
      }
    },
    [load],
  );

  const accept = useCallback(
    (id: string) => run(() => api.approveTimeEntries([id])),
    [run],
  );
  const reject = useCallback(
    (id: string) => run(() => api.rejectTimeEntry(id)),
    [run],
  );

  const setField = useCallback(
    async (field: SuggestionField, valueId: string | null): Promise<void> => {
      const id = selectedRef.current;
      if (!id) return;
      await run(() =>
        api.updateTimeEntry(
          id,
          field === "category"
            ? { categoryId: valueId ?? "" }
            : { projectId: valueId ?? "" },
        ),
      );
    },
    [run],
  );

  const toggleBillable = useCallback(async (): Promise<void> => {
    if (!detail) return;
    await run(() =>
      api.updateTimeEntry(detail.entry.id, {
        billable: !detail.entry.billable,
      }),
    );
  }, [detail, run]);

  const saveDescription = useCallback(
    async (description: string): Promise<void> => {
      if (!detail || description === "") return;
      await run(() => api.updateTimeEntry(detail.entry.id, { description }));
    },
    [detail, run],
  );

  const split = useCallback(async (): Promise<void> => {
    if (!detail) return;
    const at = splitPoint(detail.entry, detail.segments);
    try {
      const [first] = await api.splitTimeEntry(detail.entry.id, at);
      setError(null);
      setSelectedId(first.id);
      await load(first.id);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [detail, load]);

  const remove = useCallback(async (): Promise<void> => {
    if (!detail) return;
    try {
      await api.deleteTimeEntry(detail.entry.id);
      setError(null);
      setSelectedId(undefined);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [detail]);

  const retry = useCallback(async (): Promise<void> => {
    if (!detail) return;
    await run(() => api.retryClassification(detail.entry.id));
  }, [detail, run]);

  const resolveRule = useCallback(
    async (suggestion: RuleSuggestion, acceptRule: boolean): Promise<void> => {
      await run(() => api.resolveRuleSuggestion(suggestion, acceptRule));
    },
    [run],
  );

  return {
    selectedId,
    detail,
    error,
    select,
    accept,
    reject,
    setField,
    toggleBillable,
    saveDescription,
    split,
    remove,
    retry,
    resolveRule,
  };
}
