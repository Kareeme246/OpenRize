import { BarRow, Donut, type Slice } from "../../components/Charts";
import { Progress, StatCard } from "../../components/Page";
import {
  type BreakEntry,
  type BreakTotals,
  breakEnd,
  breakLabel,
  sourceLabel,
} from "../../lib/breaks";
import { formatDuration, formatTime, plural } from "../../lib/format";

interface RangeSummaryProps {
  title: string;
  /** Time in categories that count as work (and uncategorized time). */
  workMs: number;
  targetMs: number;
  targetLabel: string;
  entries: number;
  toReview: number;
  processing: number;
  categories: Slice[];
  topApps?: { app: string; ms: number }[];
  breaks?: BreakEntry[];
  breakTotals?: BreakTotals;
  scheduleLabels?: Map<string, string>;
  now?: number;
}

/**
 * The Calendar's right panel when no entry is open: work hours against the
 * target, time by category, and (for a day) top apps.
 */
export function RangeSummary({
  title,
  workMs,
  targetMs,
  targetLabel,
  entries,
  toReview,
  processing,
  categories,
  topApps,
  breaks = [],
  breakTotals,
  scheduleLabels = new Map(),
  now = Date.now(),
}: RangeSummaryProps) {
  const progress = targetMs > 0 ? workMs / targetMs : 0;
  // Under a minute reads as "0m", which only adds noise to a ranked list.
  const apps = (topApps ?? []).filter((app) => app.ms >= 60_000).slice(0, 5);
  const maxApp = apps[0]?.ms ?? 0;
  const statusLabel =
    processing > 0
      ? `Categorizing ${plural(processing, "entry", "entries")}…`
      : toReview > 0
        ? null
        : entries > 0
          ? "All caught up ✓"
          : "Nothing to review";
  return (
    <div className="flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4">
      <span className="font-semibold text-[15px] text-fg-strong">{title}</span>

      <StatCard
        label="Work hours"
        large
        value={formatDuration(workMs)}
        sub={`${Math.round(progress * 100)}% of ${targetLabel} · ${plural(entries, "entry", "entries")}`}
      >
        <div className="mt-2">
          <Progress value={progress} label="Work hours against target" />
        </div>
      </StatCard>

      {statusLabel && (
        <div className="rounded-lg border border-line bg-surface p-3 text-center text-[14px] text-fg-soft">
          {statusLabel}
        </div>
      )}

      <Donut
        slices={categories}
        title="Time by category"
        size={112}
        stacked
        large
        maxLegend={3}
        foldLone
      />

      {breakTotals && breaks.length > 0 && (
        <BreaksCard
          breaks={breaks}
          totals={breakTotals}
          scheduleLabels={scheduleLabels}
          now={now}
        />
      )}

      {apps.length > 0 && (
        <div className="space-y-2">
          <div className="font-semibold text-[12px] text-fg-faint uppercase tracking-wider">
            Top apps
          </div>
          {apps.map((app) => (
            <BarRow
              key={app.app}
              label={app.app}
              ms={app.ms}
              maxMs={maxApp}
              large
            />
          ))}
        </div>
      )}
    </div>
  );
}

const MAX_BREAK_ROWS = 6;

/** What follows a row's name: how it started, and why it did not happen. */
function detail(
  entry: BreakEntry,
  scheduleLabels: Map<string, string>,
  taken: boolean,
): string {
  const parts: string[] = [];
  const source = sourceLabel(entry.source);
  if (source !== breakLabel(entry, scheduleLabels)) parts.push(source);
  if (!taken) parts.push(entry.status);
  return parts.length > 0 ? ` · ${parts.join(" · ")}` : "";
}

/** Break history for the range: what was taken, skipped, or put off. */
function BreaksCard({
  breaks,
  totals,
  scheduleLabels,
  now,
}: {
  breaks: BreakEntry[];
  totals: BreakTotals;
  scheduleLabels: Map<string, string>;
  now: number;
}) {
  const rows = [...breaks].reverse().slice(0, MAX_BREAK_ROWS);
  const notes = [
    totals.skipped > 0 ? `${totals.skipped} skipped` : null,
    totals.missed > 0 ? `${totals.missed} missed` : null,
    totals.snoozed > 0
      ? `${plural(totals.snoozed, "snooze", "snoozes")}`
      : null,
  ].filter((note) => note !== null);
  return (
    <div className="space-y-2">
      <div className="font-semibold text-[12px] text-fg-faint uppercase tracking-wider">
        Breaks
      </div>
      <div className="rounded-lg border border-line bg-surface p-3">
        <div className="flex items-baseline justify-between gap-3">
          <span className="font-semibold text-[14px] text-fg-strong tabular-nums">
            {totals.taken > 0
              ? `${totals.taken} taken · ${formatDuration(totals.takenMs)}`
              : "None taken"}
          </span>
        </div>
        {notes.length > 0 && (
          <div className="mt-0.5 text-[12px] text-fg-soft">
            {notes.join(" · ")}
          </div>
        )}
      </div>
      <ul className="space-y-1">
        {rows.map((entry) => {
          const taken = entry.status === "taken" && entry.startedAt !== null;
          const at = entry.startedAt ?? entry.dueAt ?? 0;
          return (
            <li
              key={entry.id}
              className="flex items-center gap-2 text-[12px] text-fg-muted"
            >
              <span
                className={`size-2 shrink-0 rounded-full ${
                  taken ? "bg-break" : "bg-fg-ghost"
                }`}
              />
              <span className="min-w-0 flex-1 truncate">
                {breakLabel(entry, scheduleLabels)}
                <span className="text-fg-faint">
                  {detail(entry, scheduleLabels, taken)}
                </span>
              </span>
              <span className="shrink-0 text-fg-faint tabular-nums">
                {formatTime(at)}
                {taken &&
                  ` · ${formatDuration(Math.max(0, breakEnd(entry, now) - at))}`}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
