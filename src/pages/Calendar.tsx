import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AddTimeSheet } from "../components/AddTimeSheet";
import { AiEngineBanner } from "../components/AiEngineBanner";
import type { Slice } from "../components/Charts";
import { EntryReviewSheet } from "../components/EntryReviewSheet";
import {
  DateStepper,
  InlineError,
  PageHeader,
  ScaleControl,
} from "../components/Page";
import { useAiStatus } from "../hooks/useAiStatus";
import { useCatalog } from "../hooks/useCatalog";
import { useEntryReview } from "../hooks/useEntryReview";
import { useSettings } from "../hooks/useSettings";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { timeByApp } from "../lib/activity";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import {
  addDays,
  dayEdges,
  localDateString,
  parseLocalDate,
  rangeFor,
  rangeLabel,
  startOfWeek,
  stepDate,
} from "../lib/dates";
import {
  blockState,
  countsAsWork,
  durationOf,
  isReviewable,
  isTyping,
} from "../lib/entries";
import { formatDuration } from "../lib/format";
import { formatTargetHours, targetMsFor } from "../lib/settings";
import type {
  ActivitySegment,
  CalendarScale,
  Category,
  RollupCell,
  Route,
  TimeEntry,
} from "../lib/types";
import { DayView } from "./calendar/DayView";
import { MonthView } from "./calendar/MonthView";
import { RangeSummary } from "./calendar/RangeSummary";
import { WeekView } from "./calendar/WeekView";

type CalendarRoute = Extract<Route, { name: "calendar" }>;

interface CalendarProps {
  route: CalendarRoute;
  navigate: (route: Route) => void;
}

/** Time per category, in the categories' own order, as chart slices. */
function categorySlices(
  totals: Map<string | null, number>,
  categoryById: Map<string, Category>,
): Slice[] {
  return [...totals.entries()]
    .map(([id, ms]) => {
      const category = id ? categoryById.get(id) : undefined;
      return {
        key: id ?? "none",
        label: category?.name ?? "Uncategorized",
        color: category?.color ?? "var(--fg-ghost)",
        ms,
      };
    })
    .sort(
      (a, b) =>
        (categoryById.get(a.key)?.sort ?? Number.MAX_SAFE_INTEGER) -
        (categoryById.get(b.key)?.sort ?? Number.MAX_SAFE_INTEGER),
    );
}

export function Calendar({ route, navigate }: CalendarProps) {
  const { settings } = useSettings();
  const aiStatus = useAiStatus();
  const catalog = useCatalog();
  const { categoryById, projectById } = catalog;
  const review = useEntryReview(route.entryId);

  const scale: CalendarScale = route.scale ?? "day";
  const date = useMemo(() => parseLocalDate(route.date), [route.date]);
  const range = useMemo(() => rangeFor(scale, date), [scale, date]);
  const startMs = range.start.getTime();
  const endMs = range.end.getTime();
  // Month shows whole weeks, Monday first.
  const grid = useMemo(() => {
    const start = startOfWeek(range.start);
    const lastDay = addDays(range.end, -1);
    const end = addDays(startOfWeek(lastDay), 7);
    return {
      start,
      end,
      weeks: Math.round((end.getTime() - start.getTime()) / (7 * 86_400_000)),
    };
  }, [range]);

  const daysInRange = Math.round((endMs - startMs) / 86_400_000);

  const [entries, setEntries] = useState<TimeEntry[]>([]);
  const [segments, setSegments] = useState<ActivitySegment[]>([]);
  const [cells, setCells] = useState<RollupCell[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [adding, setAdding] = useState(false);
  const [addingAt, setAddingAt] = useState<number | undefined>();

  // Responses for a range the user already left are dropped.
  const rangeKey = `${scale}:${startMs}`;
  const shownKey = useRef(rangeKey);
  shownKey.current = rangeKey;

  /** Reads the range without rebuilding (push events land here). */
  const refresh = useCallback(async (): Promise<void> => {
    const key = `${scale}:${startMs}`;
    try {
      if (scale === "month") {
        const next = await api.entryRollup(
          { startMs: grid.start.getTime(), endMs: grid.end.getTime() },
          dayEdges(grid.start, grid.end),
          "category",
        );
        if (shownKey.current === key) setCells(next);
      } else {
        const [list, snapshot] = await Promise.all([
          api.listTimeEntries(startMs, endMs - 1),
          scale === "day" ? api.fetchActivitySnapshot(startMs) : null,
        ]);
        if (shownKey.current === key) {
          setEntries(list);
          if (snapshot) {
            setSegments(
              snapshot.segments.filter(
                (segment) =>
                  segment.startedAt >= startMs && segment.startedAt < endMs,
              ),
            );
          }
        }
      }
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  }, [scale, startMs, endMs, grid]);

  /**
   * Full load: rebuild the range's entries from its segments (a past day
   * may never have been built), then read. The rebuild emits
   * `entries-changed`, which is why `refresh` must not rebuild.
   */
  const load = useCallback(async (): Promise<void> => {
    const key = `${scale}:${startMs}`;
    setLoading(true);
    try {
      const [list, snapshot] = await Promise.all([
        api.rebuildTimeEntries(startMs, endMs - 1),
        scale === "day" ? api.fetchActivitySnapshot(startMs) : null,
      ]);
      if (shownKey.current !== key) return;
      if (scale === "month") {
        await refresh();
      } else {
        setEntries(list);
        setError(null);
      }
      // The snapshot runs to now; keep only this day's segments.
      setSegments(
        snapshot?.segments.filter(
          (segment) =>
            segment.startedAt >= startMs && segment.startedAt < endMs,
        ) ?? [],
      );
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setLoading(false);
    }
  }, [scale, startMs, endMs, refresh]);

  useEffect(() => {
    load();
  }, [load]);

  useTauriEvent(api.ENTRIES_CHANGED, () => void refresh());
  useTauriEvent(api.SUGGESTION_READY, () => void refresh());

  // Another view can open an entry or review mode here.
  const { select } = review;
  useEffect(() => {
    if (route.entryId) select(route.entryId);
  }, [route.entryId, select]);

  const visible = useMemo(
    () =>
      entries.filter(
        (entry) => entry.endedAt > startMs && entry.startedAt < endMs,
      ),
    [entries, startMs, endMs],
  );
  /** Entries waiting on the user, oldest first (review mode's queue). */
  const reviewQueue = useMemo(
    () => (scale === "month" ? [] : visible.filter(isReviewable)),
    [visible, scale],
  );
  const processingCount = useMemo(
    () => visible.filter((entry) => blockState(entry) === "processing").length,
    [visible],
  );

  const startReviewMode = useCallback((): void => {
    setReviewing(true);
    select(reviewQueue[0]?.id);
  }, [reviewQueue, select]);

  const wantsReview = useRef(route.review === true);
  useEffect(() => {
    if (route.review) wantsReview.current = true;
  }, [route.review]);
  useEffect(() => {
    if (!wantsReview.current || loading) return;
    wantsReview.current = false;
    startReviewMode();
  }, [loading, startReviewMode]);

  const go = (next: Partial<CalendarRoute>): void => {
    setReviewing(false);
    select(undefined);
    navigate({
      name: "calendar",
      scale: next.scale ?? scale,
      date: next.date ?? localDateString(date),
    });
  };

  /** After an action in review mode, move on to the next pending entry. */
  const advanceFrom = useCallback(
    async (id: string): Promise<void> => {
      const fresh = await api.listTimeEntries(startMs, endMs - 1);
      if (shownKey.current !== `${scale}:${startMs}`) return;
      setEntries(fresh);
      if (!reviewing) return;
      const queue = fresh.filter(isReviewable);
      const current = fresh.find((entry) => entry.id === id);
      const next =
        queue.find(
          (entry) =>
            entry.id !== id && current && entry.startedAt > current.startedAt,
        ) ?? queue.find((entry) => entry.id !== id);
      select(next?.id);
    },
    [scale, startMs, endMs, reviewing, select],
  );

  const accept = useCallback(
    async (id: string): Promise<void> => {
      if (await review.accept(id)) await advanceFrom(id);
    },
    [review, advanceFrom],
  );
  const reject = useCallback(
    async (id: string): Promise<void> => {
      if (await review.reject(id)) await advanceFrom(id);
    },
    [review, advanceFrom],
  );

  // Page shortcuts. The review sheet owns ⌘↵, ⌘⌫, S, and Esc; the panel
  // owns 1–9, C, P, and E.
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (isTyping(event.target)) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      const key = event.key.toLowerCase();
      if (event.key === "Escape") {
        setReviewing(false);
      } else if (key === "r" && scale !== "month") {
        event.preventDefault();
        startReviewMode();
      } else if ((key === "j" || key === "k") && visible.length > 0) {
        event.preventDefault();
        const list = reviewing ? reviewQueue : visible;
        if (list.length === 0) return;
        const index = list.findIndex((entry) => entry.id === review.selectedId);
        const step = key === "j" ? 1 : -1;
        const nextIndex =
          index === -1
            ? step === 1
              ? 0
              : list.length - 1
            : (index + step + list.length) % list.length;
        select(list[nextIndex].id);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    scale,
    visible,
    reviewQueue,
    reviewing,
    review.selectedId,
    select,
    startReviewMode,
  ]);

  const summary = useMemo(() => {
    const byCategory = new Map<string | null, number>();
    let workMs = 0;
    let reviewedMs = 0;
    let count = 0;
    let toReview = 0;
    if (scale === "month") {
      // The grid shows the neighbouring months' days too; count this one.
      const firstDay = Math.round(
        (startMs - grid.start.getTime()) / 86_400_000,
      );
      const lastDay = firstDay + daysInRange;
      for (const cell of cells) {
        if (cell.bucket < firstDay || cell.bucket >= lastDay) continue;
        const key = cell.key ?? null;
        byCategory.set(key, (byCategory.get(key) ?? 0) + cell.ms);
        if (countsAsWork(key, categoryById)) workMs += cell.ms;
        reviewedMs += cell.approvedMs;
        count += cell.entries;
        toReview += cell.pending;
      }
    } else {
      for (const entry of visible) {
        const ms = durationOf(entry);
        const key = entry.categoryId ?? null;
        byCategory.set(key, (byCategory.get(key) ?? 0) + ms);
        if (countsAsWork(key, categoryById)) workMs += ms;
        if (entry.status === "approved") reviewedMs += ms;
      }
      count = visible.length;
      toReview = reviewQueue.length;
    }
    return {
      categories: categorySlices(byCategory, categoryById),
      workMs,
      reviewedMs,
      count,
      toReview,
    };
  }, [
    scale,
    cells,
    visible,
    reviewQueue,
    categoryById,
    startMs,
    grid,
    daysInRange,
  ]);

  const reviewPosition =
    reviewing && review.selectedId
      ? (() => {
          const index = reviewQueue.findIndex(
            (entry) => entry.id === review.selectedId,
          );
          return index === -1
            ? undefined
            : { index, total: reviewQueue.length };
        })()
      : undefined;

  const title =
    scale === "day"
      ? date.toLocaleDateString(undefined, {
          weekday: "long",
          month: "long",
          day: "numeric",
        })
      : rangeLabel(scale, date);
  const targetMs = targetMsFor(settings, scale, daysInRange);

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-canvas text-fg">
      <PageHeader title={title}>
        <div className="flex w-[180px] shrink-0 justify-end">
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
        {scale === "day" && (
          <button
            type="button"
            onClick={() => {
              setAddingAt(undefined);
              setAdding(true);
            }}
            className="rounded-md border border-line bg-panel px-2.5 py-1 font-medium text-[12px] text-fg-soft hover:bg-surface hover:text-fg"
          >
            Add time
          </button>
        )}
        <DateStepper
          unit={scale}
          onStep={(direction) =>
            go({ date: localDateString(stepDate(scale, date, direction)) })
          }
          onToday={() => go({ date: localDateString(new Date()) })}
        />
        <ScaleControl
          name="calendar-scale"
          value={scale}
          onChange={(next) => go({ scale: next })}
        />
      </PageHeader>

      <AiEngineBanner status={aiStatus} />
      {error && (
        <div className="px-4 pt-3">
          <InlineError message={error} onRetry={load} />
        </div>
      )}

      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_320px] overflow-hidden">
        <div className="flex min-h-0 min-w-0 flex-col">
          {scale === "day" && (
            <DayView
              dayStart={startMs}
              entries={visible}
              segments={segments}
              loading={loading}
              selectedId={review.selectedId}
              categoryById={categoryById}
              projectById={projectById}
              onSelect={select}
              onAdd={(startMs) => {
                setAddingAt(startMs);
                setAdding(true);
              }}
            />
          )}
          {scale === "week" && (
            <WeekView
              weekStart={range.start}
              entries={visible}
              loading={loading}
              selectedId={review.selectedId}
              categoryById={categoryById}
              projectById={projectById}
              onSelect={select}
              onOpenDay={(day) => go({ scale: "day", date: day })}
            />
          )}
          {scale === "month" && (
            <MonthView
              gridStart={grid.start}
              weeks={grid.weeks}
              month={range.start.getMonth()}
              cells={cells}
              categoryById={categoryById}
              onOpenDay={(day) => go({ scale: "day", date: day })}
            />
          )}
        </div>

        <aside className="flex min-h-0 flex-col overflow-hidden border-line border-l bg-panel">
          {review.detail ? (
            <EntryReviewSheet
              review={review}
              categories={catalog.categories}
              projects={catalog.projects}
              reviewPosition={reviewPosition}
              onAccept={(id) => void accept(id)}
              onReject={(id) => void reject(id)}
              onClose={() => {
                select(undefined);
                setReviewing(false);
              }}
            />
          ) : reviewing ? (
            <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
              <div className="text-[22px] text-accent">✓</div>
              <div className="font-semibold text-[13px] text-fg-strong">
                All caught up
              </div>
              <div className="text-[11.5px] text-fg-soft">
                {formatDuration(summary.reviewedMs)} reviewed{" "}
                {scale === "day" ? "today" : "this week"}
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
            <RangeSummary
              title={
                scale === "day"
                  ? "Day summary"
                  : scale === "week"
                    ? "Week summary"
                    : "Month summary"
              }
              workMs={summary.workMs}
              targetMs={targetMs}
              targetLabel={formatTargetHours(targetMs)}
              entries={summary.count}
              toReview={summary.toReview}
              processing={processingCount}
              categories={summary.categories}
              topApps={
                scale === "day"
                  ? timeByApp(segments, Date.now()).map(({ app, ms }) => ({
                      app,
                      ms,
                    }))
                  : undefined
              }
              onStartReview={scale === "month" ? undefined : startReviewMode}
            />
          )}
        </aside>
      </div>
      {scale === "day" && adding && (
        <AddTimeSheet
          date={date}
          initialStartMs={addingAt}
          categories={catalog.categories}
          projects={catalog.projects}
          onClose={() => setAdding(false)}
          onCreated={() => void load()}
        />
      )}
    </div>
  );
}
