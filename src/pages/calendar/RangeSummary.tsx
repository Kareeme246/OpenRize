import { BarRow } from "../../components/Charts";
import { Progress, StatCard } from "../../components/Page";
import { formatDuration, plural } from "../../lib/format";
import type { Metric } from "./metrics";

interface RangeSummaryProps {
  /** Time in categories that count as work (and uncategorized time). */
  workMs: number;
  targetMs: number;
  targetLabel: string;
  entries: number;
  toReview: number;
  processing: number;
  metrics: Metric[];
  topApps?: { app: string; ms: number }[];
  onStartReview?: () => void;
}

/**
 * The Calendar panel's productivity metrics tab: work hours against the
 * target, what's left to review, headline session numbers, and (for a day)
 * top apps. Time by label lives in the Labels tab.
 */
export function RangeSummary({
  workMs,
  targetMs,
  targetLabel,
  entries,
  toReview,
  processing,
  metrics,
  topApps,
  onStartReview,
}: RangeSummaryProps) {
  const progress = targetMs > 0 ? workMs / targetMs : 0;
  // Under a minute reads as "0m", which only adds noise to a ranked list.
  const apps = (topApps ?? []).filter((app) => app.ms >= 60_000).slice(0, 5);
  const maxApp = apps[0]?.ms ?? 0;
  return (
    <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-4">
      <StatCard
        label="Work hours"
        value={formatDuration(workMs)}
        sub={`${Math.round(progress * 100)}% of ${targetLabel} target · ${plural(entries, "entry", "entries")}`}
      >
        <div className="mt-2">
          <Progress value={progress} label="Work hours against target" />
        </div>
      </StatCard>

      {toReview > 0 ? (
        <div className="flex items-center justify-between gap-2 rounded-lg border border-review/30 bg-review/10 p-3">
          <div className="min-w-0">
            <div className="font-semibold text-[12.5px] text-review">
              {plural(toReview, "entry", "entries")} to review
            </div>
            <div className="text-[11px] text-fg-soft">
              {processing > 0
                ? `${processing} more categorizing`
                : "Confirm the AI's suggestions"}
            </div>
          </div>
          {onStartReview && (
            <button
              type="button"
              onClick={onStartReview}
              className="shrink-0 rounded bg-review px-2.5 py-1 font-bold text-[11px] text-canvas hover:opacity-90"
            >
              Start
            </button>
          )}
        </div>
      ) : (
        <div className="rounded-lg border border-line bg-surface p-3 text-center text-[12px] text-fg-soft">
          {processing > 0
            ? `Categorizing ${plural(processing, "entry", "entries")}…`
            : entries > 0
              ? "All caught up ✓"
              : "Nothing to review"}
        </div>
      )}

      <div className="grid grid-cols-2 gap-2">
        {metrics.map((metric) => (
          <div
            key={metric.label}
            className="min-w-0 rounded-lg border border-line bg-surface px-3 py-2.5"
          >
            <div className="truncate font-semibold text-[10px] text-fg-faint uppercase tracking-wider">
              {metric.label}
            </div>
            <div className="mt-1 font-semibold text-[16px] text-fg-strong tabular-nums leading-tight">
              {metric.value}
            </div>
            {metric.sub && (
              <div className="mt-0.5 truncate text-[10.5px] text-fg-soft">
                {metric.sub}
              </div>
            )}
          </div>
        ))}
      </div>

      {apps.length > 0 && (
        <div className="space-y-2">
          <div className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
            Top apps
          </div>
          {apps.map((app) => (
            <BarRow key={app.app} label={app.app} ms={app.ms} maxMs={maxApp} />
          ))}
        </div>
      )}
    </div>
  );
}
