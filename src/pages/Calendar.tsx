import { useCallback, useEffect, useMemo, useState } from "react";
import * as api from "../lib/api";
import type {
  ActivitySegment,
  Category,
  EntryDetail,
  Project,
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
const HOUR_HEIGHT_PX = 64;
const TIMELINE_HEIGHT = TOTAL_HOURS * HOUR_HEIGHT_PX;
const TIMELINE_HOURS = Array.from(
  { length: TOTAL_HOURS + 1 },
  (_, i) => TIMELINE_START_HOUR + i,
);

export function Calendar({
  initialDate,
  selectedEntryId,
  onSelectEntry,
}: CalendarProps) {
  // Date state: YYYY-MM-DD
  const [dateStr, setDateStr] = useState<string>(() => {
    if (initialDate) return initialDate;
    const now = new Date();
    return now.toISOString().split("T")[0];
  });

  const [scale, setScale] = useState<"day" | "week" | "month">("day");
  const [categories, setCategories] = useState<Category[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [entries, setEntries] = useState<TimeEntry[]>([]);
  const [segments, setSegments] = useState<ActivitySegment[]>([]);
  const [selectedId, setSelectedId] = useState<string | undefined>(
    selectedEntryId,
  );
  const [detail, setDetail] = useState<EntryDetail | null>(null);
  const [detailTab, setDetailTab] = useState<
    "apps" | "titles" | "log" | "history"
  >("apps");
  const [loading, setLoading] = useState(false);
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState("");

  // Calculate day start and end in ms
  const { dayStartMs, dayEndMs } = useMemo(() => {
    const start = new Date(`${dateStr}T00:00:00`).getTime();
    const end = start + 24 * 60 * 60 * 1000 - 1;
    return { dayStartMs: start, dayEndMs: end };
  }, [dateStr]);

  // Load categories and projects
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

  // Load entries and segments for the selected day
  const loadDayData = useCallback(async () => {
    setLoading(true);
    try {
      // First trigger entry rebuilding if needed, then fetch
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

  useEffect(() => {
    loadMetadata();
  }, [loadMetadata]);

  useEffect(() => {
    loadDayData();
  }, [loadDayData]);

  // Load entry detail when selection changes
  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      return;
    }
    api
      .getEntryDetail(selectedId)
      .then((d) => {
        setDetail(d);
        setTitleDraft(d.entry.description);
      })
      .catch((err) => console.error("Failed to load detail", err));
  }, [selectedId]);

  const selectEntry = (id?: string) => {
    setSelectedId(id);
    onSelectEntry?.(id);
    setEditingTitle(false);
  };

  // Date navigation
  const prevDay = () => {
    const d = new Date(dayStartMs);
    d.setDate(d.getDate() - 1);
    setDateStr(d.toISOString().split("T")[0]);
    selectEntry(undefined);
  };

  const nextDay = () => {
    const d = new Date(dayStartMs);
    d.setDate(d.getDate() + 1);
    setDateStr(d.toISOString().split("T")[0]);
    selectEntry(undefined);
  };

  const today = () => {
    setDateStr(new Date().toISOString().split("T")[0]);
    selectEntry(undefined);
  };

  // Format header title
  const formattedDate = useMemo(() => {
    const d = new Date(`${dateStr}T12:00:00`);
    return d.toLocaleDateString(undefined, {
      weekday: "long",
      month: "long",
      day: "numeric",
    });
  }, [dateStr]);

  const pendingEntries = useMemo(
    () => entries.filter((e) => e.status === "pending"),
    [entries],
  );

  // Review mode: pick first pending entry
  const startReviewMode = () => {
    if (pendingEntries.length > 0) {
      selectEntry(pendingEntries[0].id);
    }
  };

  // Keyboard navigation & actions
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // R key: review mode
      if (e.key.toLowerCase() === "r" && !e.metaKey && !e.ctrlKey) {
        if (
          document.activeElement?.tagName !== "INPUT" &&
          document.activeElement?.tagName !== "TEXTAREA"
        ) {
          e.preventDefault();
          startReviewMode();
        }
      }

      // Escape: close panel
      if (e.key === "Escape") {
        selectEntry(undefined);
      }

      // Cmd+Enter: Accept
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        if (selectedId) {
          e.preventDefault();
          handleAccept(selectedId);
        }
      }

      // Cmd+Backspace: Reject
      if (e.key === "Backspace" && (e.metaKey || e.ctrlKey)) {
        if (selectedId) {
          e.preventDefault();
          handleReject(selectedId);
        }
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  });

  // Action handlers
  const handleAccept = async (id: string) => {
    try {
      await api.approveTimeEntries([id]);
      await loadDayData();
      // Advance to next pending entry if any
      const remaining = pendingEntries.filter((e) => e.id !== id);
      if (remaining.length > 0) {
        selectEntry(remaining[0].id);
      } else {
        selectEntry(undefined);
      }
    } catch (err) {
      console.error("Failed to approve", err);
    }
  };

  const handleReject = async (id: string) => {
    try {
      await api.rejectTimeEntry(id);
      await loadDayData();
    } catch (err) {
      console.error("Failed to reject", err);
    }
  };

  const handleCategoryChange = async (categoryId: string) => {
    if (!selectedId) return;
    try {
      const updated = await api.updateTimeEntry(selectedId, {
        categoryId: categoryId === "none" ? "" : categoryId,
      });
      setEntries((prev) =>
        prev.map((e) => (e.id === updated.id ? updated : e)),
      );
      if (detail) setDetail({ ...detail, entry: updated });
    } catch (err) {
      console.error("Failed to update category", err);
    }
  };

  const handleProjectChange = async (projectId: string) => {
    if (!selectedId) return;
    try {
      const updated = await api.updateTimeEntry(selectedId, {
        projectId: projectId === "none" ? "" : projectId,
      });
      setEntries((prev) =>
        prev.map((e) => (e.id === updated.id ? updated : e)),
      );
      if (detail) setDetail({ ...detail, entry: updated });
    } catch (err) {
      console.error("Failed to update project", err);
    }
  };

  const handleBillableToggle = async () => {
    if (!detail) return;
    try {
      const updated = await api.updateTimeEntry(detail.entry.id, {
        billable: !detail.entry.billable,
      });
      setEntries((prev) =>
        prev.map((e) => (e.id === updated.id ? updated : e)),
      );
      setDetail({ ...detail, entry: updated });
    } catch (err) {
      console.error("Failed to toggle billable", err);
    }
  };

  const handleSaveTitle = async () => {
    if (!detail) return;
    try {
      const updated = await api.updateTimeEntry(detail.entry.id, {
        description: titleDraft,
      });
      setEntries((prev) =>
        prev.map((e) => (e.id === updated.id ? updated : e)),
      );
      setDetail({ ...detail, entry: updated });
      setEditingTitle(false);
    } catch (err) {
      console.error("Failed to save description", err);
    }
  };

  const handleSplit = async () => {
    if (!detail) return;
    const midpoint = Math.floor(
      (detail.entry.startedAt + detail.entry.endedAt) / 2,
    );
    try {
      const [first] = await api.splitTimeEntry(detail.entry.id, midpoint);
      await loadDayData();
      selectEntry(first.id);
    } catch (err) {
      console.error("Failed to split entry", err);
    }
  };

  const handleDelete = async () => {
    if (!detail) return;
    try {
      await api.deleteTimeEntry(detail.entry.id);
      selectEntry(undefined);
      await loadDayData();
    } catch (err) {
      console.error("Failed to delete entry", err);
    }
  };

  // Time formatting helpers
  const formatTime = (epochMs: number) => {
    const d = new Date(epochMs);
    return d.toLocaleTimeString([], {
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    });
  };

  const formatDuration = (ms: number) => {
    const mins = Math.round(ms / 60000);
    const hrs = Math.floor(mins / 60);
    const m = mins % 60;
    if (hrs === 0) return `${m}m`;
    return `${hrs}h ${m.toString().padStart(2, "0")}m`;
  };

  // Timeline geometry: define active hours (e.g. 8 AM to 8 PM or dynamic)
  const timelineStartHour = TIMELINE_START_HOUR;
  const totalHours = TOTAL_HOURS;
  const timelineHours = TIMELINE_HOURS;
  const hourHeightPx = HOUR_HEIGHT_PX;
  const timelineHeight = TIMELINE_HEIGHT;

  const getYPosition = (epochMs: number) => {
    const d = new Date(epochMs);
    const hour = d.getHours() + d.getMinutes() / 60 + d.getSeconds() / 3600;
    const offsetHours = Math.max(0, hour - timelineStartHour);
    return (offsetHours / totalHours) * timelineHeight;
  };

  const getHeight = (startMs: number, endMs: number) => {
    const durHours = (endMs - startMs) / (3600 * 1000);
    return Math.max(28, (durHours / totalHours) * timelineHeight);
  };

  // Category lookup map
  const categoryMap = useMemo(() => {
    const map = new Map<string, Category>();
    for (const c of categories) {
      map.set(c.id, c);
    }
    return map;
  }, [categories]);

  // Project lookup map
  const projectMap = useMemo(() => {
    const map = new Map<string, Project>();
    for (const p of projects) {
      map.set(p.id, p);
    }
    return map;
  }, [projects]);

  // Compute daily totals for the summary
  const dailyTotals = useMemo(() => {
    let totalWorkMs = 0;
    const catDurations: Record<string, number> = {};
    for (const e of entries) {
      const dur = e.endedAt - e.startedAt;
      totalWorkMs += dur;
      if (e.categoryId) {
        catDurations[e.categoryId] = (catDurations[e.categoryId] || 0) + dur;
      }
    }
    return { totalWorkMs, catDurations };
  }, [entries]);

  return (
    <div className="flex h-full flex-col min-h-0 overflow-hidden bg-canvas text-fg">
      {/* Top Header */}
      <header className="flex h-12 shrink-0 items-center justify-between border-b border-line px-5">
        <div className="flex items-center gap-3">
          <h1 className="text-[15px] font-semibold text-fg-strong">
            {formattedDate}
          </h1>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={prevDay}
              className="flex size-7 items-center justify-center rounded-md border border-line bg-panel text-fg-soft hover:bg-surface hover:text-fg transition-colors"
              title="Previous day"
            >
              ‹
            </button>
            <button
              type="button"
              onClick={today}
              className="h-7 px-2.5 rounded-md border border-line bg-panel text-[12px] font-medium text-fg-soft hover:bg-surface hover:text-fg transition-colors"
            >
              Today
            </button>
            <button
              type="button"
              onClick={nextDay}
              className="flex size-7 items-center justify-center rounded-md border border-line bg-panel text-fg-soft hover:bg-surface hover:text-fg transition-colors"
              title="Next day"
            >
              ›
            </button>
          </div>
        </div>

        <div className="flex items-center gap-3">
          {/* Day / Week / Month switcher */}
          <div className="flex rounded-md border border-line bg-panel p-0.5 text-[12px] font-medium">
            <button
              type="button"
              onClick={() => setScale("day")}
              className={`rounded px-2.5 py-0.5 transition-colors ${
                scale === "day"
                  ? "bg-accent/20 text-accent font-semibold"
                  : "text-fg-soft hover:text-fg"
              }`}
            >
              Day
            </button>
            <button
              type="button"
              onClick={() => setScale("week")}
              className={`rounded px-2.5 py-0.5 transition-colors ${
                scale === "week"
                  ? "bg-accent/20 text-accent font-semibold"
                  : "text-fg-soft hover:text-fg"
              }`}
            >
              Week
            </button>
            <button
              type="button"
              onClick={() => setScale("month")}
              className={`rounded px-2.5 py-0.5 transition-colors ${
                scale === "month"
                  ? "bg-accent/20 text-accent font-semibold"
                  : "text-fg-soft hover:text-fg"
              }`}
            >
              Month
            </button>
          </div>

          {/* Review pill */}
          {pendingEntries.length > 0 && (
            <button
              type="button"
              onClick={startReviewMode}
              className="flex items-center gap-1.5 rounded-full border border-review/30 bg-review/15 px-3 py-1 text-[12px] font-semibold text-review hover:bg-review/25 transition-colors"
            >
              <span>Review {pendingEntries.length} pending</span>
              <kbd className="rounded bg-review/20 px-1 text-[10px]">R</kbd>
            </button>
          )}
        </div>
      </header>

      {/* Main Content: Timeline + Docked Review Panel */}
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_320px] overflow-hidden">
        {/* Timeline area */}
        <div className="relative flex-1 overflow-y-auto overflow-x-hidden p-4">
          <div
            className="relative grid grid-cols-[48px_16px_minmax(0,1fr)] gap-2"
            style={{ height: `${timelineHeight}px` }}
          >
            {/* 1. Time Gutter */}
            <div className="relative text-right text-[11px] font-medium text-fg-faint select-none">
              {timelineHours.map((hour) => {
                const ampm = hour >= 12 ? "PM" : "AM";
                const displayHour =
                  hour > 12 ? hour - 12 : hour === 0 ? 12 : hour;
                return (
                  <div
                    key={`gutter-hour-${hour}`}
                    className="absolute right-2 -translate-y-2"
                    style={{
                      top: `${(hour - timelineStartHour) * hourHeightPx}px`,
                    }}
                  >
                    {displayHour} {ampm}
                  </div>
                );
              })}
            </div>

            {/* 2. Activity Strip: raw segments */}
            <div className="relative rounded-full bg-surface overflow-hidden">
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
                const cat = seg.appId ? categoryMap.get(seg.appId) : undefined;
                const color = cat?.color || "var(--fg-faint)";
                return (
                  <div
                    key={seg.id}
                    title={`${seg.app}: ${seg.title}`}
                    className="absolute left-0 right-0 rounded-xs"
                    style={{
                      top: `${top}px`,
                      height: `${height}px`,
                      backgroundColor: color,
                    }}
                  />
                );
              })}
            </div>

            {/* 3. Entries Column */}
            <div className="relative">
              {/* Hour Grid Lines */}
              {timelineHours.map((hour) => (
                <div
                  key={`grid-line-${hour}`}
                  className="absolute left-0 right-0 border-b border-line-soft pointer-events-none"
                  style={{
                    top: `${(hour - timelineStartHour) * hourHeightPx}px`,
                  }}
                />
              ))}

              {/* Time Entry Blocks */}
              {entries.map((entry) => {
                const top = getYPosition(entry.startedAt);
                const height = getHeight(entry.startedAt, entry.endedAt);
                const isSelected = entry.id === selectedId;
                const cat = entry.categoryId
                  ? categoryMap.get(entry.categoryId)
                  : undefined;
                const proj = entry.projectId
                  ? projectMap.get(entry.projectId)
                  : undefined;
                const catColor = cat?.color || "var(--fg-faint)";

                const isPending = entry.status === "pending";
                const isBuilding = entry.status === "building";

                return (
                  <button
                    type="button"
                    key={entry.id}
                    onClick={() => selectEntry(entry.id)}
                    className={`text-left absolute left-0 right-4 cursor-pointer rounded-lg p-2.5 transition-all select-none border ${
                      isSelected
                        ? "ring-2 ring-accent border-accent z-20 shadow-lg"
                        : "hover:border-fg-soft/40 z-10"
                    } ${
                      isPending
                        ? "border-dashed border-review/60 bg-review/10"
                        : isBuilding
                          ? "border-dotted border-fg-soft/50 bg-surface"
                          : "border-line bg-panel shadow-xs"
                    }`}
                    style={{
                      top: `${top}px`,
                      height: `${height}px`,
                      borderLeftWidth: "4px",
                      borderLeftColor: catColor,
                    }}
                  >
                    <div className="flex h-full flex-col justify-between overflow-hidden">
                      <div className="min-w-0">
                        <div className="flex items-center gap-1.5 text-[12px] font-semibold text-fg-strong truncate">
                          <span className="truncate">{entry.description}</span>
                          {entry.status === "approved" && (
                            <span
                              className="text-accent text-[11px]"
                              title="Approved"
                            >
                              ✓
                            </span>
                          )}
                        </div>
                        <div className="text-[11px] text-fg-soft mt-0.5">
                          {formatTime(entry.startedAt)}–
                          {formatTime(entry.endedAt)} ·{" "}
                          {formatDuration(entry.endedAt - entry.startedAt)}
                        </div>
                      </div>

                      <div className="flex items-center gap-1.5 mt-1 overflow-hidden">
                        {cat && (
                          <span
                            className="inline-flex items-center gap-1 rounded-sm px-1.5 py-0.5 text-[10px] font-medium"
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
                          <span className="inline-flex items-center gap-1 rounded-sm bg-surface px-1.5 py-0.5 text-[10px] font-medium text-fg-muted">
                            <span
                              className="size-1.5 rounded-full"
                              style={{ backgroundColor: proj.color }}
                            />
                            {proj.name}
                          </span>
                        )}
                        {isPending && (
                          <span className="ml-auto text-[10px] font-semibold text-review">
                            Pending
                          </span>
                        )}
                      </div>
                    </div>
                  </button>
                );
              })}

              {entries.length === 0 && !loading && (
                <div className="flex h-48 flex-col items-center justify-center text-center text-fg-soft">
                  <p className="text-[13px]">
                    No entries recorded for this day
                  </p>
                  <p className="text-[11.5px] text-fg-faint mt-1">
                    Activity will automatically appear as you use your computer
                  </p>
                </div>
              )}
            </div>
          </div>
        </div>

        {/* Right Docked Panel: Entry Review OR Daily Summary */}
        <aside className="flex flex-col border-l border-line bg-panel overflow-hidden">
          {detail ? (
            // Entry Review Panel
            <div className="flex h-full flex-col min-h-0 overflow-hidden">
              <div className="flex items-center justify-between border-b border-line px-4 py-3">
                <span className="text-[13px] font-semibold text-fg-strong">
                  Review entry
                </span>
                <button
                  type="button"
                  onClick={() => selectEntry(undefined)}
                  className="rounded p-1 text-fg-faint hover:bg-surface hover:text-fg transition-colors"
                  title="Close (Esc)"
                >
                  ✕
                </button>
              </div>

              <div className="flex-1 overflow-y-auto p-4 space-y-4">
                {/* Time range & Billable */}
                <div className="flex items-center justify-between text-[12px] text-fg-soft">
                  <span>
                    {formatTime(detail.entry.startedAt)}–
                    {formatTime(detail.entry.endedAt)} ·{" "}
                    {formatDuration(
                      detail.entry.endedAt - detail.entry.startedAt,
                    )}
                  </span>
                  <button
                    type="button"
                    onClick={handleBillableToggle}
                    className={`rounded px-2 py-0.5 text-[11.5px] font-medium transition-colors ${
                      detail.entry.billable
                        ? "bg-accent/20 text-accent font-semibold"
                        : "bg-surface text-fg-faint hover:text-fg"
                    }`}
                  >
                    $ Billable
                  </button>
                </div>

                {/* Description editing */}
                <div className="rounded-lg bg-surface p-2.5 border border-line">
                  {editingTitle ? (
                    <div className="space-y-2">
                      <textarea
                        value={titleDraft}
                        onChange={(e) => setTitleDraft(e.target.value)}
                        className="w-full rounded bg-canvas px-2 py-1.5 text-[12.5px] text-fg outline-hidden border border-line focus:border-accent"
                        rows={2}
                      />
                      <div className="flex justify-end gap-1.5">
                        <button
                          type="button"
                          onClick={() => setEditingTitle(false)}
                          className="rounded px-2 py-0.5 text-[11px] text-fg-soft hover:text-fg"
                        >
                          Cancel
                        </button>
                        <button
                          type="button"
                          onClick={handleSaveTitle}
                          className="rounded bg-accent px-2.5 py-0.5 text-[11px] font-semibold text-accent-fg"
                        >
                          Save
                        </button>
                      </div>
                    </div>
                  ) : (
                    <button
                      type="button"
                      onClick={() => setEditingTitle(true)}
                      className="w-full text-left cursor-pointer text-[12.5px] text-fg-strong font-medium hover:text-accent transition-colors"
                      title="Click to edit description"
                    >
                      {detail.entry.description}
                    </button>
                  )}
                </div>

                {/* Category Dropdown */}
                <div className="space-y-1">
                  <label
                    htmlFor="review-category-select"
                    className="text-[11px] font-semibold text-fg-faint uppercase"
                  >
                    Category
                  </label>
                  <select
                    id="review-category-select"
                    value={detail.entry.categoryId || "none"}
                    onChange={(e) => handleCategoryChange(e.target.value)}
                    className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-[12px] text-fg outline-hidden focus:border-accent"
                  >
                    <option value="none">No Category</option>
                    {categories.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.name}
                      </option>
                    ))}
                  </select>
                </div>

                {/* Project Dropdown */}
                <div className="space-y-1">
                  <label
                    htmlFor="review-project-select"
                    className="text-[11px] font-semibold text-fg-faint uppercase"
                  >
                    Project
                  </label>
                  <select
                    id="review-project-select"
                    value={detail.entry.projectId || "none"}
                    onChange={(e) => handleProjectChange(e.target.value)}
                    className="w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-[12px] text-fg outline-hidden focus:border-accent"
                  >
                    <option value="none">No Project</option>
                    {projects.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                </div>

                {/* Accept / Reject actions */}
                <div className="flex items-center gap-2 pt-2 border-t border-line">
                  <button
                    type="button"
                    onClick={() => handleAccept(detail.entry.id)}
                    className="flex-1 rounded-md bg-accent px-3 py-1.5 text-[12px] font-semibold text-accent-fg hover:opacity-90 transition-opacity"
                  >
                    Accept <span className="opacity-70 text-[10px]">⌘↵</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => handleReject(detail.entry.id)}
                    className="rounded-md border border-line bg-surface px-3 py-1.5 text-[12px] font-medium text-fg-soft hover:bg-surface-strong hover:text-fg transition-colors"
                  >
                    Reject <span className="opacity-70 text-[10px]">⌘⌫</span>
                  </button>
                  <button
                    type="button"
                    onClick={handleSplit}
                    className="rounded-md border border-line bg-surface px-2 py-1.5 text-[12px] text-fg-soft hover:bg-surface-strong hover:text-fg transition-colors"
                    title="Split in half"
                  >
                    Split
                  </button>
                  <button
                    type="button"
                    onClick={handleDelete}
                    className="rounded-md border border-line bg-surface px-2 py-1.5 text-[12px] text-danger hover:bg-danger/10 transition-colors"
                    title="Delete entry"
                  >
                    ✕
                  </button>
                </div>

                {/* Review Tabs: Apps, Titles, Log, History */}
                <div className="pt-2">
                  <div className="flex border-b border-line text-[11px] font-medium">
                    <button
                      type="button"
                      onClick={() => setDetailTab("apps")}
                      className={`flex-1 pb-1.5 text-center transition-colors border-b-2 ${
                        detailTab === "apps"
                          ? "border-accent text-accent font-semibold"
                          : "border-transparent text-fg-soft hover:text-fg"
                      }`}
                    >
                      Apps
                    </button>
                    <button
                      type="button"
                      onClick={() => setDetailTab("titles")}
                      className={`flex-1 pb-1.5 text-center transition-colors border-b-2 ${
                        detailTab === "titles"
                          ? "border-accent text-accent font-semibold"
                          : "border-transparent text-fg-soft hover:text-fg"
                      }`}
                    >
                      Titles
                    </button>
                    <button
                      type="button"
                      onClick={() => setDetailTab("log")}
                      className={`flex-1 pb-1.5 text-center transition-colors border-b-2 ${
                        detailTab === "log"
                          ? "border-accent text-accent font-semibold"
                          : "border-transparent text-fg-soft hover:text-fg"
                      }`}
                    >
                      Log
                    </button>
                    <button
                      type="button"
                      onClick={() => setDetailTab("history")}
                      className={`flex-1 pb-1.5 text-center transition-colors border-b-2 ${
                        detailTab === "history"
                          ? "border-accent text-accent font-semibold"
                          : "border-transparent text-fg-soft hover:text-fg"
                      }`}
                    >
                      History
                    </button>
                  </div>

                  <div className="pt-3 text-[11.5px]">
                    {detailTab === "apps" && (
                      <div className="space-y-2">
                        {detail.apps.map((appItem) => (
                          <div key={appItem.app} className="space-y-1">
                            <div className="flex justify-between text-fg-soft">
                              <span>{appItem.app}</span>
                              <span className="font-mono text-fg-faint">
                                {formatDuration(appItem.durationMs)} (
                                {Math.round(appItem.percentage)}%)
                              </span>
                            </div>
                            <div className="h-1.5 w-full rounded-full bg-surface overflow-hidden">
                              <div
                                className="h-full bg-accent rounded-full"
                                style={{ width: `${appItem.percentage}%` }}
                              />
                            </div>
                          </div>
                        ))}
                      </div>
                    )}

                    {detailTab === "titles" && (
                      <div className="space-y-2 max-h-48 overflow-y-auto">
                        {detail.titles.map((t) => (
                          <div
                            key={`${t.app}-${t.startedAt}-${t.title.slice(0, 20)}`}
                            className="border-b border-line-soft pb-1"
                          >
                            <div className="text-fg-strong truncate">
                              {t.title}
                            </div>
                            <div className="text-[10px] text-fg-faint">
                              {t.app} · {formatDuration(t.durationMs)}
                            </div>
                          </div>
                        ))}
                        {detail.titles.length === 0 && (
                          <div className="text-fg-faint text-center py-2">
                            No titles captured
                          </div>
                        )}
                      </div>
                    )}

                    {detailTab === "log" && (
                      <div className="space-y-1 max-h-48 overflow-y-auto font-mono text-[10.5px]">
                        {detail.segments.map((s) => (
                          <div
                            key={s.id}
                            className="flex justify-between text-fg-soft"
                          >
                            <span className="truncate pr-2">
                              {formatTime(s.startedAt)} {s.app}
                            </span>
                            <span className="shrink-0 text-fg-faint">
                              {s.endedAt
                                ? formatDuration(s.endedAt - s.startedAt)
                                : "active"}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}

                    {detailTab === "history" && (
                      <div className="space-y-1 max-h-48 overflow-y-auto text-[11px]">
                        {detail.events.map((evt) => (
                          <div
                            key={evt.id}
                            className="flex items-center justify-between text-fg-soft"
                          >
                            <span>
                              {evt.kind.toUpperCase()} by {evt.actor}
                            </span>
                            <span className="text-[10px] text-fg-faint font-mono">
                              {formatTime(evt.at)}
                            </span>
                          </div>
                        ))}
                        {detail.events.length === 0 && (
                          <div className="text-fg-faint text-center py-2">
                            No audit events
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                </div>
              </div>
            </div>
          ) : (
            // Daily Summary (When no entry selected)
            <div className="flex h-full flex-col min-h-0 overflow-y-auto p-4 space-y-5">
              <span className="text-[13px] font-semibold text-fg-strong">
                Day summary
              </span>

              {/* Work against 8h target */}
              <div className="rounded-lg bg-surface p-3 border border-line space-y-2">
                <div className="flex justify-between text-[12px]">
                  <span className="text-fg-soft">Tracked time</span>
                  <span className="font-semibold text-fg-strong">
                    {formatDuration(dailyTotals.totalWorkMs)} / 8h
                  </span>
                </div>
                <div className="h-2 w-full rounded-full bg-surface-strong overflow-hidden">
                  <div
                    className="h-full bg-accent rounded-full transition-all"
                    style={{
                      width: `${Math.min(
                        100,
                        (dailyTotals.totalWorkMs / (8 * 3600 * 1000)) * 100,
                      )}%`,
                    }}
                  />
                </div>
              </div>

              {/* Pending count banner */}
              {pendingEntries.length > 0 ? (
                <div className="flex items-center justify-between rounded-lg border border-review/30 bg-review/10 p-3">
                  <div>
                    <div className="text-[12.5px] font-semibold text-review">
                      {pendingEntries.length} entries to review
                    </div>
                    <div className="text-[11px] text-fg-soft">
                      Assign category and projects
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={startReviewMode}
                    className="rounded bg-review px-2.5 py-1 text-[11px] font-bold text-canvas hover:opacity-90"
                  >
                    Start
                  </button>
                </div>
              ) : (
                <div className="rounded-lg border border-line bg-surface p-3 text-[12px] text-fg-soft text-center">
                  All caught up for this day ✓
                </div>
              )}

              {/* Categories breakdown */}
              <div className="space-y-2">
                <span className="text-[11px] font-semibold text-fg-faint uppercase">
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
                              className="size-2 rounded-full shrink-0"
                              style={{
                                backgroundColor:
                                  cat?.color || "var(--fg-faint)",
                              }}
                            />
                            <span className="truncate text-fg-muted">
                              {cat?.name || "Unassigned"}
                            </span>
                          </div>
                          <span className="font-mono text-fg-soft shrink-0">
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
