import { BarRow, Donut, type Slice } from "../../components/Charts";
import { Progress, StatCard } from "../../components/Page";
import { formatDuration, plural } from "../../lib/format";

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
  onStartReview?: () => void;
}

/**
 * The Calendar's right panel when no entry is open: work hours against the
 * target, what's left to review, time by category, and (for a day) top apps.
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
  onStartReview,
}: RangeSummaryProps) {
  const progress = targetMs > 0 ? workMs / targetMs : 0;
  // Under a minute reads as "0m", which only adds noise to a ranked list.
  const apps = (topApps ?? []).filter((app) => app.ms >= 60_000).slice(0, 5);
  const maxApp = apps[0]?.ms ?? 0;
  return (
    <div className="flex h-full min-h-0 flex-col gap-4 overflow-y-auto p-4">
      <span className="font-semibold text-[13px] text-fg-strong">{title}</span>

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

      <Donut slices={categories} title="Time by category" size={112} stacked />

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
