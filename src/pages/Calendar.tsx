import { listen } from "@tauri-apps/api/event";
import {
  type CSSProperties,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { AiEngineBanner } from "../components/AiEngineBanner";
import { EntryReviewPanel } from "../components/EntryReviewPanel";
import { useAiStatus } from "../hooks/useAiStatus";
import { useSettings } from "../hooks/useSettings";
import * as api from "../lib/api";
import {
  BAND_LABEL,
  BAND_TONE,
  band,
  entryConfidence,
  PREFILL,
  percent,
} from "../lib/confidence";
import type {
  ActivitySegment,
  Category,
  EntryDetail,
  Project,
  RuleSuggestion,
  SuggestionField,
  TimeEntry,
} from "../lib/types";

interface CalendarProps {
  initialDate?: string;
  selectedEntryId?: string;
  onSelectEntry?: (id?: string) => void;
}

const TIMELINE_START_HOUR = 8;
const TIMELINE_END_HOUR = 20;
const TOTAL_HOURS = TIMELINE_END_HOUR - TIMELINE_START_HOUR;
const HOUR_HEIGHT_PX = 88;
const TIMELINE_HEIGHT = TOTAL_HOURS * HOUR_HEIGHT_PX;
const TIMELINE_HOURS = Array.from(
  { length: TOTAL_HOURS + 1 },
  (_, i) => TIMELINE_START_HOUR + i,
);

/** How a block renders; see the design board's Calendar states. */
type BlockState =
  | "building"
  | "processing"
  | "failed"
  | "needsYou"
  | "pending"
  | "approved";

function blockState(entry: TimeEntry): BlockState {
  if (entry.status === "approved") return "approved";
  if (entry.status === "building") return "building";
  const job = entry.ai?.state;
  if (entry.status === "processing" || job === "queued" || job === "running") {
    return "processing";
  }
  const confidence = entryConfidence(
    entry.ai?.categoryConfidence,
    entry.ai?.projectConfidence,
  );
  if (job === "failed" && confidence === undefined) return "failed";
  if (!entry.categoryId || (confidence !== undefined && confidence < PREFILL)) {
    return "needsYou";
  }
  return "pending";
}

/** How much of a block's content fits its height. */
type Density = "full" | "compact" | "line" | "sliver";

const DENSITY_CLASSES: Record<Density, string> = {
  full: "rounded-lg p-2.5",
  compact: "rounded-lg px-2.5 py-1",
  line: "rounded-md px-2 py-0",
  sliver: "rounded-sm p-0",
};

const REVIEWABLE: BlockState[] = ["pending", "needsYou", "failed"];

/** Local YYYY-MM-DD (toISOString would give the UTC date). */
function localDateString(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

function isTyping(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  return (
    element?.tagName === "INPUT" ||
    element?.tagName === "TEXTAREA" ||
    element?.tagName === "SELECT" ||
    element?.isContentEditable === true
  );
}

function blockStyle(state: BlockState, color: string): CSSProperties {
  switch (state) {
    case "pending":
      return {
        backgroundImage: `repeating-linear-gradient(135deg, color-mix(in srgb, ${color} 14%, var(--bg-panel)) 0 6px, color-mix(in srgb, ${color} 7%, var(--bg-panel)) 6px 12px)`,
      };
    case "needsYou":
    case "failed":
      return {
        backgroundImage:
          "repeating-linear-gradient(135deg, var(--bg-surface-1) 0 6px, transparent 6px 12px)",
      };
    default:
      return {};
  }
}

const BLOCK_CLASSES: Record<BlockState, string> = {
  approved: "border-line bg-panel shadow-xs",
  pending: "border-dashed border-review/60",
  needsYou: "border-dashed border-line",
  failed: "border-dashed border-danger/50",
  processing: "entry-processing border-line",
  building: "border-dotted border-fg-soft/50 bg-transparent",
};

export function Calendar({
  initialDate,
  selectedEntryId,
  onSelectEntry,
}: CalendarProps) {
  const { settings } = useSettings();
  const aiStatus = useAiStatus();
  const suggestProjects = settings.aiSuggest === "categoryProject";

  const [dateStr, setDateStr] = useState<string>(
    () => initialDate ?? localDateString(new Date()),
  );
  const [scale, setScale] = useState<"day" | "week" | "month">("day");
  const [categories, setCategories] = useState<Category[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [entries, setEntries] = useState<TimeEntry[]>([]);
  const [segments, setSegments] = useState<ActivitySegment[]>([]);
  const [selectedId, setSelectedId] = useState<string | undefined>(
    selectedEntryId,
  );
  const [detail, setDetail] = useState<EntryDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [reviewing, setReviewing] = useState(false);

  const { dayStartMs, dayEndMs } = useMemo(() => {
    const start = new Date(`${dateStr}T00:00:00`).getTime();
    return { dayStartMs: start, dayEndMs: start + 24 * 60 * 60 * 1000 - 1 };
  }, [dateStr]);

  const loadMetadata = useCallback(async () => {
    try {
      const [cats, projs] = await Promise.all([
        api.listCategories(),
        api.listProjects(),
      ]);
      setCategories(cats);
      setProjects(projs);
    } catch (err) {
      console.error("Failed to load metadata", err);
    }
  }, []);

  /** Full load for a day: rebuild its entries, then read them back. */
  const loadDayData = useCallback(async () => {
    setLoading(true);
    try {
      const [entryList, snapshot] = await Promise.all([
        api.rebuildTimeEntries(dayStartMs, dayEndMs),
        api.fetchActivitySnapshot(dayStartMs),
      ]);
      setEntries(entryList);
      setSegments(snapshot.segments);
    } catch (err) {
      console.error("Failed to load day data", err);
    } finally {
      setLoading(false);
    }
  }, [dayStartMs, dayEndMs]);

  /**
   * Re-reads entries without rebuilding. Used for push events: a rebuild
   * itself emits `entries-changed`, so rebuilding here would loop.
   */
  const refreshEntries = useCallback(async () => {
    try {
      setEntries(await api.listTimeEntries(dayStartMs, dayEndMs));
    } catch (err) {
      console.error("Failed to refresh entries", err);
    }
  }, [dayStartMs, dayEndMs]);

  const refreshDetail = useCallback(async (id: string | undefined) => {
    if (!id) {
      setDetail(null);
      return;
    }
    try {
      setDetail(await api.getEntryDetail(id));
    } catch (err) {
      console.error("Failed to load detail", err);
      setDetail(null);
    }
  }, []);

  useEffect(() => {
    loadMetadata();
  }, [loadMetadata]);

  useEffect(() => {
    loadDayData();
  }, [loadDayData]);

  useEffect(() => {
    refreshDetail(selectedId);
  }, [selectedId, refreshDetail]);

  // Suggestions arrive in the background: patch the day and the open panel.
  const selectedRef = useRef(selectedId);
  selectedRef.current = selectedId;
  useEffect(() => {
    const onEntries = listen(api.ENTRIES_CHANGED, () => {
      refreshEntries();
      refreshDetail(selectedRef.current);
    });
    const onSuggestion = listen<{ entryId: string }>(
      api.SUGGESTION_READY,
      (event) => {
        refreshEntries();
        if (event.payload.entryId === selectedRef.current) {
          refreshDetail(selectedRef.current);
        }
      },
    );
    return () => {
      void onEntries.then((stop) => stop());
      void onSuggestion.then((stop) => stop());
    };
  }, [refreshEntries, refreshDetail]);

  const selectEntry = useCallback(
    (id?: string) => {
      setSelectedId(id);
      onSelectEntry?.(id);
    },
    [onSelectEntry],
  );

  const goToDate = (date: Date): void => {
    setDateStr(localDateString(date));
    selectEntry(undefined);
    setReviewing(false);
  };
  const prevDay = () => {
    const d = new Date(dayStartMs);
    d.setDate(d.getDate() - 1);
    goToDate(d);
  };
  const nextDay = () => {
    const d = new Date(dayStartMs);
    d.setDate(d.getDate() + 1);
    goToDate(d);
  };
  const today = () => goToDate(new Date());

  const formattedDate = useMemo(() => {
    const d = new Date(`${dateStr}T12:00:00`);
    return d.toLocaleDateString(undefined, {
      weekday: "long",
      month: "long",
      day: "numeric",
    });
  }, [dateStr]);

  /** Entries waiting on the user, oldest first (review mode's queue). */
  const reviewQueue = useMemo(
    () => entries.filter((e) => REVIEWABLE.includes(blockState(e))),
    [entries],
  );
  const processingCount = useMemo(
    () => entries.filter((e) => blockState(e) === "processing").length,
    [entries],
  );

  const startReviewMode = useCallback(() => {
    setReviewing(true);
    selectEntry(reviewQueue[0]?.id);
  }, [reviewQueue, selectEntry]);

  /** After an action, move to the next entry still waiting for review. */
  const advanceFrom = useCallback(
    async (id: string) => {
      const fresh = await api.listTimeEntries(dayStartMs, dayEndMs);
      setEntries(fresh);
      if (!reviewing) {
        await refreshDetail(id);
        return;
      }
      const queue = fresh.filter((e) => REVIEWABLE.includes(blockState(e)));
      const current = fresh.find((e) => e.id === id);
      const next =
        queue.find(
          (e) => e.id !== id && current && e.startedAt > current.startedAt,
        ) ?? queue.find((e) => e.id !== id);
      selectEntry(next?.id);
    },
    [dayStartMs, dayEndMs, reviewing, refreshDetail, selectEntry],
  );

  const handleAccept = useCallback(
    async (id: string) => {
      try {
        await api.approveTimeEntries([id]);
        await advanceFrom(id);
      } catch (err) {
        console.error("Failed to approve", err);
      }
    },
    [advanceFrom],
  );

  const handleReject = useCallback(
    async (id: string) => {
      try {
        await api.rejectTimeEntry(id);
        await advanceFrom(id);
      } catch (err) {
        console.error("Failed to reject", err);
      }
    },
    [advanceFrom],
  );

  /** Adopts a mutated entry, keeping the AI summary only lists carry. */
  const applyUpdate = useCallback((updated: TimeEntry): void => {
    setEntries((prev) =>
      prev.map((e) => (e.id === updated.id ? { ...updated, ai: e.ai } : e)),
    );
  }, []);

  const handleSetField = useCallback(
    async (field: SuggestionField, valueId: string | null) => {
      if (!selectedId) return;
      try {
        const updated = await api.updateTimeEntry(
          selectedId,
          field === "category"
            ? { categoryId: valueId ?? "" }
            : { projectId: valueId ?? "" },
        );
        applyUpdate(updated);
        // The rule prompt depends on the new value; re-read the panel.
        await refreshDetail(selectedId);
      } catch (err) {
        console.error(`Failed to set ${field}`, err);
      }
    },
    [selectedId, refreshDetail, applyUpdate],
  );

  const handleBillableToggle = async () => {
    if (!detail) return;
    try {
      const updated = await api.updateTimeEntry(detail.entry.id, {
        billable: !detail.entry.billable,
      });
      applyUpdate(updated);
      await refreshDetail(detail.entry.id);
    } catch (err) {
      console.error("Failed to toggle billable", err);
    }
  };

  const handleSaveDescription = async (description: string) => {
    if (!detail || description === "") return;
    try {
      const updated = await api.updateTimeEntry(detail.entry.id, {
        description,
      });
      applyUpdate(updated);
      await refreshDetail(detail.entry.id);
    } catch (err) {
      console.error("Failed to save description", err);
    }
  };

  /** Splits at the app switch nearest the middle, else at the midpoint. */
  const handleSplit = useCallback(async () => {
    if (!detail) return;
    const { startedAt, endedAt } = detail.entry;
    const midpoint = Math.floor((startedAt + endedAt) / 2);
    let splitAt = midpoint;
    let best = Number.POSITIVE_INFINITY;
    detail.segments.forEach((segment, index) => {
      const previous = detail.segments[index - 1];
      if (!previous || previous.app === segment.app) return;
      if (segment.startedAt <= startedAt || segment.startedAt >= endedAt)
        return;
      const distance = Math.abs(segment.startedAt - midpoint);
      if (distance < best) {
        best = distance;
        splitAt = segment.startedAt;
      }
    });
    try {
      const [first] = await api.splitTimeEntry(detail.entry.id, splitAt);
      await refreshEntries();
      selectEntry(first.id);
      await refreshDetail(first.id);
    } catch (err) {
      console.error("Failed to split entry", err);
    }
  }, [detail, refreshEntries, refreshDetail, selectEntry]);

  const handleDelete = async () => {
    if (!detail) return;
    try {
      await api.deleteTimeEntry(detail.entry.id);
      selectEntry(undefined);
      await refreshEntries();
    } catch (err) {
      console.error("Failed to delete entry", err);
    }
  };

  const handleRetry = async () => {
    if (!detail) return;
    try {
      await api.retryClassification(detail.entry.id);
      await refreshEntries();
      await refreshDetail(detail.entry.id);
    } catch (err) {
      console.error("Failed to retry classification", err);
    }
  };

  const handleResolveRule = async (
    suggestion: RuleSuggestion,
    accept: boolean,
  ) => {
    try {
      await api.resolveRuleSuggestion(suggestion, accept);
      await refreshDetail(selectedId);
    } catch (err) {
      console.error("Failed to resolve rule suggestion", err);
    }
  };

  // Page-level shortcuts. The review panel owns 1-9, C, P, and E.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (isTyping(event.target)) return;
      const command = event.metaKey || event.ctrlKey;
      const key = event.key.toLowerCase();

      if (event.key === "Escape") {
        selectEntry(undefined);
        setReviewing(false);
        return;
      }
      if (command && event.key === "Enter" && selectedId) {
        event.preventDefault();
        if (detail?.entry.categoryId && detail.entry.status !== "approved") {
          handleAccept(selectedId);
        }
        return;
      }
      if (command && event.key === "Backspace" && selectedId) {
        event.preventDefault();
        if (detail?.entry.status !== "approved") handleReject(selectedId);
        return;
      }
      if (command || event.altKey) return;

      if (key === "r") {
        event.preventDefault();
        startReviewMode();
      } else if ((key === "j" || key === "k") && entries.length > 0) {
        event.preventDefault();
        const list = reviewing ? reviewQueue : entries;
        if (list.length === 0) return;
        const index = list.findIndex((e) => e.id === selectedId);
        const step = key === "j" ? 1 : -1;
        const nextIndex =
          index === -1
            ? step === 1
              ? 0
              : list.length - 1
            : (index + step + list.length) % list.length;
        selectEntry(list[nextIndex].id);
      } else if (key === "s" && detail) {
        event.preventDefault();
        handleSplit();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    detail,
    entries,
    handleAccept,
    handleReject,
    handleSplit,
    reviewQueue,
    reviewing,
    selectEntry,
    selectedId,
    startReviewMode,
  ]);

  const formatTime = (epochMs: number) =>
    new Date(epochMs).toLocaleTimeString([], {
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    });

  const formatDuration = (ms: number) => {
    const mins = Math.round(ms / 60000);
    const hrs = Math.floor(mins / 60);
    const m = mins % 60;
    if (hrs === 0) return `${m}m`;
    return `${hrs}h ${m.toString().padStart(2, "0")}m`;
  };

  const getYPosition = (epochMs: number) => {
    const d = new Date(epochMs);
    const hour = d.getHours() + d.getMinutes() / 60 + d.getSeconds() / 3600;
    const offsetHours = Math.max(0, hour - TIMELINE_START_HOUR);
    return (offsetHours / TOTAL_HOURS) * TIMELINE_HEIGHT;
  };

  const getHeight = (startMs: number, endMs: number) => {
    const durHours = (endMs - startMs) / (3600 * 1000);
    // Height follows duration, less a hairline so neighbours don't touch.
    return Math.max(4, (durHours / TOTAL_HOURS) * TIMELINE_HEIGHT - 2);
  };

  const categoryMap = useMemo(
    () => new Map(categories.map((c) => [c.id, c])),
    [categories],
  );
  const projectMap = useMemo(
    () => new Map(projects.map((p) => [p.id, p])),
    [projects],
  );

  const dailyTotals = useMemo(() => {
    let totalWorkMs = 0;
    let reviewedMs = 0;
    const catDurations: Record<string, number> = {};
    for (const e of entries) {
      const dur = e.endedAt - e.startedAt;
      totalWorkMs += dur;
      if (e.status === "approved") reviewedMs += dur;
      if (e.categoryId) {
        catDurations[e.categoryId] = (catDurations[e.categoryId] || 0) + dur;
      }
    }
    return { totalWorkMs, reviewedMs, catDurations };
  }, [entries]);

  const reviewPosition =
    reviewing && selectedId
      ? (() => {
          const index = reviewQueue.findIndex((e) => e.id === selectedId);
          return index === -1
            ? undefined
            : { index, total: reviewQueue.length };
        })()
      : undefined;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-canvas text-fg">
      <header className="flex h-12 shrink-0 items-center justify-between border-line border-b px-5">
        <div className="flex items-center gap-3">
          <h1 className="font-semibold text-[15px] text-fg-strong">
            {formattedDate}
          </h1>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={prevDay}
              className="flex size-7 items-center justify-center rounded-md border border-line bg-panel text-fg-soft transition-colors hover:bg-surface hover:text-fg"
              title="Previous day"
              aria-label="Previous day"
            >
              ‹
            </button>
            <button
              type="button"
              onClick={today}
              className="h-7 rounded-md border border-line bg-panel px-2.5 font-medium text-[12px] text-fg-soft transition-colors hover:bg-surface hover:text-fg"
            >
              Today
            </button>
            <button
              type="button"
              onClick={nextDay}
              className="flex size-7 items-center justify-center rounded-md border border-line bg-panel text-fg-soft transition-colors hover:bg-surface hover:text-fg"
              title="Next day"
              aria-label="Next day"
            >
              ›
            </button>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <div className="flex rounded-md border border-line bg-panel p-0.5 font-medium text-[12px]">
            {(["day", "week", "month"] as const).map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => setScale(option)}
                className={`rounded px-2.5 py-0.5 capitalize transition-colors ${
                  scale === option
                    ? "bg-accent/20 font-semibold text-accent"
                    : "text-fg-soft hover:text-fg"
                }`}
              >
                {option}
              </button>
            ))}
          </div>

          {reviewQueue.length > 0 && (
            <button
              type="button"
              onClick={startReviewMode}
              className="flex items-center gap-1.5 rounded-full border border-review/30 bg-review/15 px-3 py-1 font-semibold text-[12px] text-review transition-colors hover:bg-review/25"
            >
              <span>Review {reviewQueue.length} pending</span>
              <kbd className="rounded bg-review/20 px-1 text-[10px]">R</kbd>
            </button>
          )}
        </div>
      </header>

      <AiEngineBanner status={aiStatus} />

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_320px] overflow-hidden">
        <div className="relative flex-1 overflow-y-auto overflow-x-hidden p-4">
          <div
            className="relative grid grid-cols-[48px_16px_minmax(0,1fr)] gap-2"
            style={{ height: `${TIMELINE_HEIGHT}px` }}
          >
            {/* 1. Time gutter */}
            <div className="relative select-none text-right font-medium text-[11px] text-fg-faint">
              {TIMELINE_HOURS.map((hour) => {
                const ampm = hour >= 12 ? "PM" : "AM";
                const displayHour =
                  hour > 12 ? hour - 12 : hour === 0 ? 12 : hour;
                return (
                  <div
                    key={`gutter-hour-${hour}`}
                    className="absolute right-2 -translate-y-2"
                    style={{
                      top: `${(hour - TIMELINE_START_HOUR) * HOUR_HEIGHT_PX}px`,
                    }}
                  >
                    {displayHour} {ampm}
                  </div>
                );
              })}
            </div>

            {/* 2. Activity strip: raw segments */}
            <div className="relative overflow-hidden rounded-full bg-surface">
              {segments.map((seg) => {
                if (seg.kind === "break") return null;
                const top = getYPosition(seg.startedAt);
                const height = Math.max(
                  3,
                  getHeight(
                    seg.startedAt,
                    seg.endedAt || seg.startedAt + 60000,
                  ),
                );
                const owner = seg.entryId
                  ? entries.find((e) => e.id === seg.entryId)
                  : undefined;
                const cat = owner?.categoryId
                  ? categoryMap.get(owner.categoryId)
                  : undefined;
                return (
                  <div
                    key={seg.id}
                    title={`${seg.app}: ${seg.title}`}
                    className="absolute right-0 left-0 rounded-xs"
                    style={{
                      top: `${top}px`,
                      height: `${height}px`,
                      backgroundColor: cat?.color || "var(--fg-faint)",
                    }}
                  />
                );
              })}
            </div>

            {/* 3. Entries column */}
            <div className="relative">
              {TIMELINE_HOURS.map((hour) => (
                <div
                  key={`grid-line-${hour}`}
                  className="pointer-events-none absolute right-0 left-0 border-line-soft border-b"
                  style={{
                    top: `${(hour - TIMELINE_START_HOUR) * HOUR_HEIGHT_PX}px`,
                  }}
                />
              ))}

              {entries.map((entry) => {
                const state = blockState(entry);
                const top = getYPosition(entry.startedAt);
                const height = getHeight(entry.startedAt, entry.endedAt);
                const isSelected = entry.id === selectedId;
                const cat = entry.categoryId
                  ? categoryMap.get(entry.categoryId)
                  : undefined;
                const proj = entry.projectId
                  ? projectMap.get(entry.projectId)
                  : undefined;
                const railColor =
                  state === "processing" || state === "needsYou" || !cat
                    ? "var(--fg-faint)"
                    : cat.color;
                const confidence = entryConfidence(
                  entry.ai?.categoryConfidence,
                  entry.ai?.projectConfidence,
                );
                const unconfirmed = state === "pending";
                const density: Density =
                  height >= 58
                    ? "full"
                    : height >= 26
                      ? "compact"
                      : height >= 12
                        ? "line"
                        : "sliver";
                const timeText = `${formatTime(entry.startedAt)}–${formatTime(entry.endedAt)} · ${
                  state === "building"
                    ? "building"
                    : formatDuration(entry.endedAt - entry.startedAt)
                }`;

                const title = (
                  <span
                    className={`truncate ${state === "processing" ? "font-normal text-fg-soft" : ""}`}
                  >
                    {state === "processing"
                      ? "Categorizing…"
                      : entry.description}
                  </span>
                );
                const approvedMark = state === "approved" && (
                  <span
                    className="shrink-0 text-[11px] text-accent"
                    title={`Approved by ${entry.approvedBy ?? "you"}`}
                  >
                    {entry.approvedBy === "auto" || entry.approvedBy === "rule"
                      ? `✓ ${entry.approvedBy}`
                      : "✓"}
                  </span>
                );
                const status = (
                  <span className="ml-auto shrink-0 font-semibold text-[10px]">
                    {state === "needsYou" && (
                      <span className="text-review">
                        {BAND_LABEL.low}
                        {confidence !== undefined &&
                          ` · ${percent(confidence)}`}
                      </span>
                    )}
                    {state === "pending" && confidence !== undefined && (
                      <span
                        className={`tabular-nums ${BAND_TONE[band(confidence)]}`}
                      >
                        {percent(confidence)}
                      </span>
                    )}
                    {state === "pending" && confidence === undefined && (
                      <span className="text-review">Pending</span>
                    )}
                    {state === "failed" && (
                      <span className="text-danger">
                        Couldn't categorize · Retry
                      </span>
                    )}
                  </span>
                );
                const chips = state !== "processing" && (
                  <>
                    {cat && (
                      <span
                        className="inline-flex shrink-0 items-center gap-1 rounded-sm px-1.5 py-0.5 font-medium text-[10px]"
                        style={{
                          backgroundColor: `color-mix(in srgb, ${cat.color} 15%, transparent)`,
                          color: cat.color,
                        }}
                      >
                        <span
                          className="size-1.5 rounded-full"
                          style={{ backgroundColor: cat.color }}
                        />
                        {cat.name}
                      </span>
                    )}
                    {proj && (
                      <span className="inline-flex min-w-0 items-center gap-1 rounded-sm bg-surface px-1.5 py-0.5 font-medium text-[10px] text-fg-muted">
                        <span
                          className="size-1.5 shrink-0 rounded-full"
                          style={{ backgroundColor: proj.color }}
                        />
                        <span className="truncate">
                          {proj.name}
                          {unconfirmed && "?"}
                        </span>
                      </span>
                    )}
                  </>
                );

                return (
                  <button
                    type="button"
                    key={entry.id}
                    onClick={() => selectEntry(entry.id)}
                    title={
                      density === "full"
                        ? undefined
                        : `${entry.description}\n${timeText}`
                    }
                    className={`absolute right-4 left-0 cursor-pointer select-none overflow-hidden border text-left transition-all ${
                      DENSITY_CLASSES[density]
                    } ${
                      isSelected
                        ? "z-20 shadow-lg ring-2 ring-accent"
                        : "z-10 hover:border-fg-soft/40"
                    } ${BLOCK_CLASSES[state]}`}
                    style={{
                      top: `${top}px`,
                      height: `${height}px`,
                      borderLeftWidth: "4px",
                      borderLeftStyle: "solid",
                      borderLeftColor: railColor,
                      ...blockStyle(state, railColor),
                    }}
                  >
                    {density === "full" && (
                      <div className="flex h-full flex-col justify-between overflow-hidden">
                        <div className="min-w-0">
                          <div className="flex items-center gap-1.5 font-semibold text-[12px] text-fg-strong">
                            {title}
                            {approvedMark}
                          </div>
                          <div className="mt-0.5 truncate text-[11px] text-fg-soft">
                            {timeText}
                          </div>
                        </div>
                        <div className="flex items-center gap-1.5 overflow-hidden">
                          {chips}
                          {status}
                        </div>
                      </div>
                    )}
                    {density === "compact" && (
                      <div className="flex h-full flex-col justify-center gap-0.5 overflow-hidden">
                        <div className="flex items-center gap-1.5 font-semibold text-[11.5px] text-fg-strong">
                          {title}
                          {approvedMark}
                          {height < 40 && status}
                        </div>
                        {height >= 40 && (
                          <div className="flex items-center gap-1.5 overflow-hidden text-[10.5px] text-fg-soft">
                            <span className="shrink-0">{timeText}</span>
                            {chips}
                            {status}
                          </div>
                        )}
                      </div>
                    )}
                    {density === "line" && (
                      <div className="flex h-full items-center gap-1.5 overflow-hidden font-semibold text-[10.5px] text-fg-strong leading-none">
                        {title}
                        {approvedMark}
                        {status}
                      </div>
                    )}
                  </button>
                );
              })}

              {entries.length === 0 && !loading && (
                <div className="flex h-48 flex-col items-center justify-center text-center text-fg-soft">
                  <p className="text-[13px]">
                    No entries recorded for this day
                  </p>
                  <p className="mt-1 text-[11.5px] text-fg-faint">
                    Activity will automatically appear as you use your computer
                  </p>
                </div>
              )}
            </div>
          </div>
        </div>

        <aside className="flex flex-col overflow-hidden border-line border-l bg-panel">
          {detail ? (
            <EntryReviewPanel
              detail={detail}
              categories={categories}
              projects={projects}
              suggestProjects={suggestProjects}
              reviewPosition={reviewPosition}
              onClose={() => {
                selectEntry(undefined);
                setReviewing(false);
              }}
              onAccept={() => handleAccept(detail.entry.id)}
              onReject={() => handleReject(detail.entry.id)}
              onSplit={handleSplit}
              onDelete={handleDelete}
              onRetry={handleRetry}
              onSetField={handleSetField}
              onToggleBillable={handleBillableToggle}
              onSaveDescription={handleSaveDescription}
              onResolveRule={handleResolveRule}
              formatTime={formatTime}
              formatDuration={formatDuration}
            />
          ) : reviewing ? (
            <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
              <div className="text-[22px] text-accent">✓</div>
              <div className="font-semibold text-[13px] text-fg-strong">
                All caught up
              </div>
              <div className="text-[11.5px] text-fg-soft">
                {formatDuration(dailyTotals.reviewedMs)} reviewed today
                {processingCount > 0 &&
                  ` · ${processingCount} still categorizing`}
              </div>
              <button
                type="button"
                onClick={() => setReviewing(false)}
                className="mt-2 rounded-md border border-line px-3 py-1 text-[11.5px] text-fg-soft hover:bg-surface hover:text-fg"
              >
                Back to summary
              </button>
            </div>
          ) : (
            <div className="flex h-full min-h-0 flex-col space-y-5 overflow-y-auto p-4">
              <span className="font-semibold text-[13px] text-fg-strong">
                Day summary
              </span>

              <div className="space-y-2 rounded-lg border border-line bg-surface p-3">
                <div className="flex justify-between text-[12px]">
                  <span className="text-fg-soft">Tracked time</span>
                  <span className="font-semibold text-fg-strong">
                    {formatDuration(dailyTotals.totalWorkMs)} / 8h
                  </span>
                </div>
                <div className="h-2 w-full overflow-hidden rounded-full bg-surface-strong">
                  <div
                    className="h-full rounded-full bg-accent transition-all"
                    style={{
                      width: `${Math.min(
                        100,
                        (dailyTotals.totalWorkMs / (8 * 3600 * 1000)) * 100,
                      )}%`,
                    }}
                  />
                </div>
              </div>

              {reviewQueue.length > 0 ? (
                <div className="flex items-center justify-between rounded-lg border border-review/30 bg-review/10 p-3">
                  <div>
                    <div className="font-semibold text-[12.5px] text-review">
                      {reviewQueue.length}{" "}
                      {reviewQueue.length === 1 ? "entry" : "entries"} to review
                    </div>
                    <div className="text-[11px] text-fg-soft">
                      {processingCount > 0
                        ? `${processingCount} more categorizing`
                        : "Confirm the AI's suggestions"}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={startReviewMode}
                    className="rounded bg-review px-2.5 py-1 font-bold text-[11px] text-canvas hover:opacity-90"
                  >
                    Start
                  </button>
                </div>
              ) : (
                <div className="rounded-lg border border-line bg-surface p-3 text-center text-[12px] text-fg-soft">
                  {processingCount > 0
                    ? `Categorizing ${processingCount} ${processingCount === 1 ? "entry" : "entries"}…`
                    : "All caught up for this day ✓"}
                </div>
              )}

              <div className="space-y-2">
                <span className="font-semibold text-[11px] text-fg-faint uppercase">
                  Time by category
                </span>
                <div className="space-y-1.5">
                  {Object.entries(dailyTotals.catDurations).map(
                    ([catId, dur]) => {
                      const cat = categoryMap.get(catId);
                      return (
                        <div
                          key={catId}
                          className="flex items-center justify-between text-[11.5px]"
                        >
                          <div className="flex items-center gap-1.5 truncate">
                            <span
                              className="size-2 shrink-0 rounded-full"
                              style={{
                                backgroundColor:
                                  cat?.color || "var(--fg-faint)",
                              }}
                            />
                            <span className="truncate text-fg-muted">
                              {cat?.name || "Unassigned"}
                            </span>
                          </div>
                          <span className="shrink-0 font-mono text-fg-soft">
                            {formatDuration(dur)}
                          </span>
                        </div>
                      );
                    },
                  )}
                  {Object.keys(dailyTotals.catDurations).length === 0 && (
                    <div className="text-[11.5px] text-fg-faint">
                      No categories assigned yet
                    </div>
                  )}
                </div>
              </div>
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}
