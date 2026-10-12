import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { entryEnd, splitPoint } from "../lib/entries";
import type {
  EntryDetail,
  RuleSuggestion,
  SuggestionField,
} from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

/**
 * Where the split flow is: picking the cut on a timeline, or confirming it in
 * the split dialog. `at` is the cut so far.
 */
export interface SplitStage {
  stage: "pick" | "confirm";
  at: number;
}

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
  /** Returns the open approved entry to pending so it can be edited. */
  unapprove: () => Promise<void>;
  setField: (field: SuggestionField, valueId: string | null) => Promise<void>;
  toggleBillable: () => Promise<void>;
  saveDescription: (description: string) => Promise<void>;
  setTimes: (startedAt: number, endedAt: number) => Promise<void>;
  splitting: SplitStage | null;
  /**
   * Starts splitting the open entry (or entry `id`, once it has opened), at
   * its smart split point: on a timeline by picking the cut there (`pick`),
   * elsewhere straight in the split dialog.
   */
  beginSplit: (pick: boolean, id?: string) => void;
  /** The cut picked on the timeline; moves on to the dialog. */
  pickSplit: (at: number) => void;
  cancelSplit: () => void;
  /** Splits the open entry at `at`; resolves true once split. */
  split: (at: number) => Promise<boolean>;
  /** Merges the open entry with an adjacent entry. */
  merge: (otherId: string) => Promise<void>;
  remove: () => Promise<void>;
  removeField: (field: SuggestionField, id: string) => Promise<void>;
  retry: () => Promise<void>;
  resolveRule: (suggestion: RuleSuggestion, accept: boolean) => Promise<void>;
}

/** The smart split point, on the minute like every cut the user picks. */
function suggestedCut(detail: EntryDetail): number {
  return (
    Math.round(splitPoint(detail.entry, detail.segments) / 60_000) * 60_000
  );
}

export function useEntryReview(initialId?: string): EntryReview {
  const [selectedId, setSelectedId] = useState<string | undefined>(initialId);
  const [detail, setDetail] = useState<EntryDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [splitting, setSplitting] = useState<SplitStage | null>(null);
  const pendingSplit = useRef<{ id: string; pick: boolean } | null>(null);
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
      if (selectedRef.current !== id) return;
      // Usually the entry is gone: an undo can take away the open entry (an
      // added entry, a split's second half). The panel closes either way.
      console.warn("entry review: could not open", id, describeError(cause));
      setDetail(null);
      setSplitting(null);
      setSelectedId(undefined);
    }
  }, []);

  useEffect(() => {
    setError(null);
    setSplitting(null);
    load(selectedId);
  }, [selectedId, load]);

  // A split asked for before its entry had opened starts once it has.
  useEffect(() => {
    const pending = pendingSplit.current;
    if (!pending || detail?.entry.id !== pending.id) return;
    pendingSplit.current = null;
    setSplitting({
      stage: pending.pick ? "pick" : "confirm",
      at: suggestedCut(detail),
    });
  }, [detail]);

  // Suggestions land in the background: patch the open panel.
  useTauriEvent(api.ENTRIES_CHANGED, () => void load(selectedRef.current));
  useTauriEvent<{ entryId: string }>(api.SUGGESTION_READY, (payload) => {
    if (payload.entryId === selectedRef.current) void load(selectedRef.current);
  });

  const select = useCallback((id?: string) => {
    pendingSplit.current = null;
    setSplitting(null);
    setSelectedId(id);
  }, []);

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

  const unapprove = useCallback(async (): Promise<void> => {
    if (!detail) return;
    await run(() => api.unapproveTimeEntries([detail.entry.id]));
  }, [detail, run]);

  const setField = useCallback(
    async (field: SuggestionField, valueId: string | null): Promise<void> => {
      const id = selectedRef.current;
      if (!id) return;
      await run(() =>
        api.updateTimeEntry(
          id,
          field === "category"
            ? {
                categoryId:
                  valueId === detail?.entry.categoryId ? "" : (valueId ?? ""),
              }
            : {
                projectId:
                  valueId === detail?.entry.projectId ? "" : (valueId ?? ""),
              },
        ),
      );
    },
    [run, detail],
  );

  const removeField = useCallback(
    async (field: SuggestionField, id: string): Promise<void> => {
      await run(() =>
        field === "category" ? api.deleteCategory(id) : api.deleteProject(id),
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

  const setTimes = useCallback(
    async (startedAt: number, endedAt: number): Promise<void> => {
      if (!detail) return;
      await run(() =>
        api.updateTimeEntry(detail.entry.id, { startedAt, endedAt }),
      );
    },
    [detail, run],
  );

  const beginSplit = useCallback(
    (pick: boolean, id?: string): void => {
      const target = id ?? selectedRef.current;
      if (!target) return;
      pendingSplit.current = { id: target, pick };
      if (target !== selectedRef.current) {
        setSplitting(null);
        setSelectedId(target);
      } else if (detail?.entry.id === target) {
        pendingSplit.current = null;
        setError(null);
        setSplitting({
          stage: pick ? "pick" : "confirm",
          at: suggestedCut(detail),
        });
      }
    },
    [detail],
  );

  const pickSplit = useCallback((at: number): void => {
    setError(null);
    setSplitting({ stage: "confirm", at });
  }, []);

  const cancelSplit = useCallback((): void => {
    pendingSplit.current = null;
    setError(null);
    setSplitting(null);
  }, []);

  const split = useCallback(
    async (at: number): Promise<boolean> => {
      if (!detail) return false;
      if (at <= detail.entry.startedAt || at >= entryEnd(detail.entry)) {
        setError("Split point must be inside the entry");
        return false;
      }
      try {
        const [first] = await api.splitTimeEntry(detail.entry.id, at);
        setError(null);
        setSplitting(null);
        setSelectedId(first.id);
        await load(first.id);
        return true;
      } catch (cause) {
        setError(describeError(cause));
        return false;
      }
    },
    [detail, load],
  );

  const merge = useCallback(
    async (otherId: string): Promise<void> => {
      if (!detail) return;
      try {
        const merged = await api.mergeTimeEntries(detail.entry.id, otherId);
        setError(null);
        setSelectedId(merged.id);
        await load(merged.id);
      } catch (cause) {
        setError(describeError(cause));
      }
    },
    [detail, load],
  );

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
    unapprove,
    setField,
    toggleBillable,
    saveDescription,
    setTimes,
    splitting,
    beginSplit,
    pickSplit,
    cancelSplit,
    split,
    merge,
    remove,
    removeField,
    retry,
    resolveRule,
  };
}
