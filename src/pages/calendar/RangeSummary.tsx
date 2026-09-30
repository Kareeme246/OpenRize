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
  processing: number;
  categories: Slice[];
  topApps?: { app: string; ms: number }[];
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
  processing,
  categories,
  topApps,
}: RangeSummaryProps) {
  const progress = targetMs > 0 ? workMs / targetMs : 0;
  // Under a minute reads as "0m", which only adds noise to a ranked list.
  const apps = (topApps ?? []).filter((app) => app.ms >= 60_000).slice(0, 5);
  const maxApp = apps[0]?.ms ?? 0;
  const statusLabel =
    processing > 0
      ? `Categorizing ${plural(processing, "entry", "entries")}…`
      : null;
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
