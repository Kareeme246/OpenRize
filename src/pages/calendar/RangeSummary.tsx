import { BarRow, Donut, type Slice } from "../../components/Charts";
import { Progress, StatCard } from "../../components/Page";
import { Tooltip } from "../../components/Tooltip";
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

/** Apps listed on their own; the rest fold into one "Other" row. */
const TOP_APPS = 4;

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
  const ranked = (topApps ?? []).filter((app) => app.ms >= 60_000);
  // Folding a single app into "Other" would only hide its name.
  const keep = ranked.length > TOP_APPS + 1 ? TOP_APPS : ranked.length;
  const apps = ranked.slice(0, keep);
  const rest = ranked.slice(keep);
  const restMs = rest.reduce((sum, app) => sum + app.ms, 0);
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
          {rest.length > 0 && (
            <Tooltip
              placement="left"
              content={
                <div className="space-y-0.5">
                  {rest.map((app) => (
                    <div key={app.app} className="flex justify-between gap-4">
                      <span className="truncate">{app.app}</span>
                      <span className="tabular-nums">
                        {formatDuration(app.ms)}
                      </span>
                    </div>
                  ))}
                </div>
              }
            >
              <div>
                <BarRow
                  label={`Other (${rest.length})`}
                  ms={restMs}
                  maxMs={Math.max(maxApp, restMs)}
                  color="var(--fg-ghost)"
                  large
                />
              </div>
            </Tooltip>
          )}
        </div>
      )}
    </div>
  );
}
