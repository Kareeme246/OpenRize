import { useState } from "react";
import { BarRow, Donut, type Slice } from "../../components/Charts";
import { Dot, EmptyState, type TabOption } from "../../components/Page";
import { useTimers } from "../../hooks/useTimers";
import * as api from "../../lib/api";
import { describeError } from "../../lib/api";
import { formatDuration as formatShare } from "../../lib/format";
import { elapsedMs, formatDuration, type Timer } from "../../lib/timers";

export type PanelTab = "labels" | "tasks" | "metrics";

const PANEL_TABS: PanelTab[] = ["labels", "tasks", "metrics"];
const STORAGE_KEY = "openrize.calendar.panelTab";

/** The Calendar panel's tabs; metrics is an icon at the far end. */
export const PANEL_TAB_OPTIONS: TabOption<PanelTab>[] = [
  { value: "labels", label: "Labels" },
  { value: "tasks", label: "Tasks" },
  {
    value: "metrics",
    label: "Productivity metrics",
    icon: (
      <>
        <path d="M3 3v16a2 2 0 0 0 2 2h16" />
        <path d="M18 17V9" />
        <path d="M13 17V5" />
        <path d="M8 17v-3" />
      </>
    ),
  },
];

/**
 * The open tab, remembered on this Mac. Storage can be unavailable (private
 * or cleared site data), so it only ever falls back to Labels.
 */
export function usePanelTab(): [PanelTab, (tab: PanelTab) => void] {
  const [tab, setTab] = useState<PanelTab>(() => {
    try {
      const saved = window.localStorage.getItem(STORAGE_KEY);
      return PANEL_TABS.find((value) => value === saved) ?? "labels";
    } catch {
      return "labels";
    }
  });
  const choose = (next: PanelTab): void => {
    setTab(next);
    try {
      window.localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // Remembering the tab is a convenience; the switch itself happened.
    }
  };
  return [tab, choose];
}

/**
 * Where the range's time went, by the labels on its sessions: categories
 * as a ring, then projects as ranked bars with the share that has one.
 */
export function LabelsTab({
  categories,
  projects,
}: {
  categories: Slice[];
  /** "No project" time uses the key `none`. */
  projects: Slice[];
}) {
  const totalMs = projects.reduce((sum, slice) => sum + slice.ms, 0);
  const assignedMs = projects
    .filter((slice) => slice.key !== "none")
    .reduce((sum, slice) => sum + slice.ms, 0);
  const ranked = projects
    .filter((slice) => slice.ms > 0)
    .sort(
      (a, b) =>
        Number(a.key === "none") - Number(b.key === "none") || b.ms - a.ms,
    );
  const maxMs = Math.max(0, ...ranked.map((slice) => slice.ms));
  if (totalMs <= 0) {
    return (
      <div className="flex min-h-0 flex-1 flex-col p-4">
        <EmptyState
          title="No labeled time yet"
          hint="Categories and projects on this range's sessions add up here."
        />
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-4">
      <Donut slices={categories} title="Time by category" size={112} stacked />

      <section className="space-y-2">
        <div className="flex items-baseline justify-between gap-2">
          <span className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
            Time by project
          </span>
          <span className="font-mono text-[10.5px] text-fg-faint tabular-nums">
            {Math.round((assignedMs / totalMs) * 100)}% assigned
          </span>
        </div>
        {ranked.map((slice) => (
          <BarRow
            key={slice.key}
            label={
              <span className="flex min-w-0 items-center gap-1.5">
                <Dot color={slice.color} />
                <span className="truncate">{slice.label}</span>
              </span>
            }
            ms={slice.ms}
            maxMs={maxMs}
            color={slice.color}
            trailing={formatShare(slice.ms)}
          />
        ))}
      </section>
    </div>
  );
}

/** Running first, then newest, so what is ticking is always on top. */
function byActivity(a: Timer, b: Timer): number {
  const running = Number(b.startedAt !== null) - Number(a.startedAt !== null);
  return running || b.createdAt - a.createdAt;
}

/**
 * Manual tasks (the Timers page's trackers), started and paused beside the
 * day they happen in. Renaming, resetting, and deleting stay on that page.
 */
export function TasksTab({ onManage }: { onManage: () => void }) {
  const timers = useTimers();
  const [label, setLabel] = useState("");
  const [error, setError] = useState<string | null>(null);
  const trimmed = label.trim();

  // Naming a task means starting on it, so a new one starts at once.
  const add = async (): Promise<void> => {
    if (trimmed.length === 0) return;
    setLabel("");
    try {
      const list = await api.createTimer(trimmed);
      const created = list.reduce<Timer | undefined>(
        (newest, timer) =>
          newest === undefined || timer.createdAt >= newest.createdAt
            ? timer
            : newest,
        undefined,
      );
      if (created) timers.start(created.id);
      setError(null);
    } catch (cause) {
      setError(describeError(cause));
    }
  };

  const list = [...timers.timers].sort(byActivity);
  const problem = error ?? timers.error;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void add();
        }}
        className="flex gap-2"
      >
        <input
          value={label}
          onChange={(event) => setLabel(event.target.value)}
          placeholder="What are you working on?"
          aria-label="New task name"
          className="min-w-0 flex-1 rounded-lg border border-line bg-inset px-2.5 py-1.5 text-[12px] text-fg-strong outline-none placeholder:text-fg-faint focus:border-accent/50"
        />
        <button
          type="submit"
          disabled={trimmed.length === 0}
          className="shrink-0 rounded-lg bg-accent px-3 py-1.5 font-semibold text-[11.5px] text-accent-fg transition-opacity hover:opacity-90 disabled:opacity-40"
        >
          Start
        </button>
      </form>

      {problem && (
        <p role="alert" className="text-[11.5px] text-danger">
          {problem}
        </p>
      )}

      {!timers.loading && list.length === 0 ? (
        <EmptyState
          title="No tasks yet"
          hint="Name what you're working on to time it by hand, alongside automatic tracking."
        />
      ) : (
        <ul className="flex flex-col gap-1.5">
          {list.map((timer) => (
            <TaskRow
              key={timer.id}
              timer={timer}
              now={timers.now}
              onToggle={() =>
                timer.startedAt === null
                  ? timers.start(timer.id)
                  : timers.pause(timer.id)
              }
            />
          ))}
        </ul>
      )}

      <button
        type="button"
        onClick={onManage}
        className="mt-auto self-start text-[11.5px] text-fg-soft transition-colors hover:text-accent"
      >
        Manage tasks in Timers →
      </button>
    </div>
  );
}

function TaskRow({
  timer,
  now,
  onToggle,
}: {
  timer: Timer;
  now: number;
  onToggle: () => void;
}) {
  const running = timer.startedAt !== null;
  return (
    <li
      className={`flex items-center gap-2.5 rounded-lg border px-2.5 py-2 transition-colors ${
        running ? "border-accent/30 bg-accent-soft" : "border-line bg-surface"
      }`}
    >
      <button
        type="button"
        onClick={onToggle}
        aria-label={running ? `Pause ${timer.label}` : `Start ${timer.label}`}
        title={running ? "Pause" : "Start"}
        className={`grid size-6 shrink-0 place-items-center rounded-full transition-colors ${
          running
            ? "bg-accent text-accent-fg hover:opacity-90"
            : "bg-surface-strong text-fg-soft hover:text-fg"
        }`}
      >
        <svg viewBox="0 0 24 24" className="size-3" aria-hidden="true">
          {running ? (
            <path fill="currentColor" d="M7 5h3.5v14H7zM13.5 5H17v14h-3.5z" />
          ) : (
            <path fill="currentColor" d="M8 5.5v13l10.5-6.5z" />
          )}
        </svg>
      </button>
      <span
        className={`min-w-0 flex-1 truncate text-[12px] ${
          running ? "font-semibold text-fg-strong" : "text-fg-muted"
        }`}
      >
        {timer.label}
      </span>
      <span
        className={`shrink-0 font-mono text-[11.5px] tabular-nums ${
          running ? "text-accent" : "text-fg-faint"
        }`}
      >
        {formatDuration(elapsedMs(timer, now))}
      </span>
    </li>
  );
}
