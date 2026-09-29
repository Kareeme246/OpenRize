import type { ReactNode } from "react";
import { formatDuration, plural } from "../../lib/format";
import type { SheetSummary, SheetTally } from "../../lib/timesheetGrid";

function Glyph({ children }: { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="size-3.5 shrink-0"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

const CHECK_CIRCLE = (
  <Glyph>
    <circle cx="12" cy="12" r="9" />
    <path d="m8.5 12 2.5 2.5 4.5-5" />
  </Glyph>
);

function Figure({
  icon,
  label,
  tally,
  first,
}: {
  icon: ReactNode;
  label: string;
  tally: SheetTally;
  first?: boolean;
}) {
  return (
    <div className={`min-w-0 px-5 ${first ? "" : "border-line border-l"}`}>
      <div className="flex items-center gap-1.5 font-medium text-[11px] text-fg-soft uppercase tracking-wider">
        {icon}
        {label}
      </div>
      <div className="mt-1.5 font-semibold text-[22px] text-fg-strong tabular-nums leading-tight">
        {formatDuration(tally.ms)}
      </div>
      <div className="mt-0.5 text-[11.5px] text-fg-faint">
        {plural(tally.entries, "entry", "entries")}
      </div>
    </div>
  );
}

/** Beyond this many entries the bar stops drawing one segment each. */
const MAX_SEGMENTS = 40;

/**
 * My Timesheet's day header from Rise: pending, approved, and total time in
 * one card, then a review progress bar with a segment per entry.
 */
export function DaySummary({ summary }: { summary: SheetSummary }) {
  const reviewable = summary.review.entries + summary.approved.entries;
  const segments = Math.min(reviewable, MAX_SEGMENTS);
  const filled =
    reviewable > 0
      ? Math.round((summary.approved.entries / reviewable) * segments)
      : 0;
  return (
    <section
      aria-label="Day summary"
      className="rounded-xl border border-line bg-panel py-4"
    >
      <div className="grid grid-cols-3">
        <Figure
          first
          icon={
            <Glyph>
              <path d="M7 3h10M7 21h10M8 3c0 5 8 5 8 9s-8 4-8 9M16 3c0 5-8 5-8 9" />
            </Glyph>
          }
          label="Pending review"
          tally={summary.review}
        />
        <Figure icon={CHECK_CIRCLE} label="Approved" tally={summary.approved} />
        <Figure
          icon={
            <Glyph>
              <circle cx="12" cy="12" r="9" />
              <path d="M12 7v5l3 2" />
            </Glyph>
          }
          label="Time entry hours"
          tally={summary.total}
        />
      </div>
      <div className="mx-5 mt-4 border-line border-t pt-3">
        <div className="flex items-center justify-between text-[12px]">
          <span className="flex items-center gap-1.5 font-medium text-fg">
            <span className="text-success">{CHECK_CIRCLE}</span>
            Review progress
          </span>
          <span className="text-fg-soft tabular-nums">
            {summary.approved.entries}/{reviewable} approved
          </span>
        </div>
        <div
          role="progressbar"
          aria-label="Approved entries"
          aria-valuemin={0}
          aria-valuemax={reviewable}
          aria-valuenow={summary.approved.entries}
          className="mt-2 flex gap-1"
        >
          {segments === 0 ? (
            <span className="h-1 flex-1 rounded-full bg-surface-strong" />
          ) : (
            Array.from({ length: segments }, (_, index) => index).map(
              (index) => (
                <span
                  key={`segment-${index}`}
                  className={`h-1 flex-1 rounded-full ${
                    index < filled ? "bg-success" : "bg-surface-strong"
                  }`}
                />
              ),
            )
          )}
        </div>
      </div>
    </section>
  );
}
