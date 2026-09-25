import { type Slice, StackedBar } from "../../components/Charts";
import { addDays, isSameDay, localDateString } from "../../lib/dates";
import { formatDuration, formatHours } from "../../lib/format";
import type { Category, RollupCell } from "../../lib/types";

interface MonthViewProps {
  /** The Monday the grid starts on. */
  gridStart: Date;
  weeks: number;
  month: number;
  /** A category rollup with one bucket per grid day. */
  cells: RollupCell[];
  categoryById: Map<string, Category>;
  onOpenDay: (date: string) => void;
}

export interface DayTotal {
  ms: number;
  pending: number;
  slices: Slice[];
}

/** Folds a per-day category rollup into one total per grid day. */
export function dayTotals(
  cells: RollupCell[],
  categoryById: Map<string, Category>,
  days: number,
): DayTotal[] {
  const totals: DayTotal[] = Array.from({ length: days }, () => ({
    ms: 0,
    pending: 0,
    slices: [],
  }));
  for (const cell of cells) {
    const day = totals[cell.bucket];
    if (!day) continue;
    const category = cell.key ? categoryById.get(cell.key) : undefined;
    day.ms += cell.ms;
    day.pending += cell.pending;
    day.slices.push({
      key: cell.key ?? "none",
      label: category?.name ?? "Uncategorized",
      color: category?.color ?? "var(--fg-ghost)",
      ms: cell.ms,
    });
  }
  for (const day of totals) {
    // Fixed order by category, so a colour sits in the same place every day.
    day.slices.sort((a, b) => {
      const left = categoryById.get(a.key)?.sort ?? Number.MAX_SAFE_INTEGER;
      const right = categoryById.get(b.key)?.sort ?? Number.MAX_SAFE_INTEGER;
      return left - right;
    });
  }
  return totals;
}

/**
 * Each day cell shows its total, a stacked category bar, and "N to review".
 * Titles are left to the Day view, which a click opens: Rize's truncated
 * entry titles were unreadable at this size.
 */
export function MonthView({
  gridStart,
  weeks,
  month,
  cells,
  categoryById,
  onOpenDay,
}: MonthViewProps) {
  const days = weeks * 7;
  const totals = dayTotals(cells, categoryById, days);
  const today = new Date();
  const weekdays = Array.from({ length: 7 }, (_, index) =>
    addDays(gridStart, index).toLocaleDateString(undefined, {
      weekday: "short",
    }),
  );

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-4">
      <div className="grid grid-cols-7 gap-1.5 pb-1.5">
        {weekdays.map((weekday) => (
          <div
            key={weekday}
            className="px-2 font-medium text-[11px] text-fg-faint uppercase"
          >
            {weekday}
          </div>
        ))}
      </div>
      <div
        className="grid min-h-[420px] flex-1 grid-cols-7 gap-1.5"
        style={{ gridTemplateRows: `repeat(${weeks}, minmax(84px, 1fr))` }}
      >
        {totals.map((total, index) => {
          const day = addDays(gridStart, index);
          const inMonth = day.getMonth() === month;
          const isToday = isSameDay(day, today);
          const future = day.getTime() > today.getTime();
          const label = day.toLocaleDateString(undefined, {
            weekday: "long",
            month: "long",
            day: "numeric",
          });
          return (
            <button
              key={day.getTime()}
              type="button"
              onClick={() => onOpenDay(localDateString(day))}
              aria-label={`${label}: ${
                total.ms > 0 ? formatDuration(total.ms) : "nothing tracked"
              }${total.pending > 0 ? `, ${total.pending} to review` : ""}`}
              className={`@container flex min-w-0 flex-col gap-1.5 rounded-lg border p-2 text-left transition-colors hover:border-fg-soft/40 ${
                inMonth
                  ? "border-line bg-panel"
                  : "border-line-soft bg-transparent"
              } ${isToday ? "ring-1 ring-accent/60" : ""}`}
            >
              <div className="flex items-center justify-between gap-1">
                <span
                  className={`font-semibold text-[12px] tabular-nums ${
                    isToday
                      ? "rounded-full bg-accent px-1.5 text-accent-fg"
                      : inMonth
                        ? "text-fg-strong"
                        : "text-fg-faint"
                  }`}
                >
                  {day.getDate()}
                </span>
                {total.ms > 0 && (
                  <span
                    className={`font-mono text-[11.5px] tabular-nums ${
                      inMonth ? "text-fg-muted" : "text-fg-faint"
                    }`}
                  >
                    {formatHours(total.ms)}
                  </span>
                )}
              </div>
              {total.ms > 0 && <StackedBar slices={total.slices} />}
              <div className="mt-auto">
                {total.pending > 0 && !future && (
                  <span
                    title={`${total.pending} to review`}
                    className="inline-block max-w-full truncate whitespace-nowrap rounded-full bg-review/15 px-1.5 py-0.5 font-semibold text-[10.5px] text-review"
                  >
                    {total.pending}
                    {/* A narrow cell keeps just the count. */}
                    <span className="hidden @[7.5rem]:inline"> to review</span>
                  </span>
                )}
              </div>
            </button>
          );
        })}
      </div>
    </div>
  );
}
