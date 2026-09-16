import type { ReactNode } from "react";
import {
  type ActivitySegment,
  formatClock,
  KIND_STYLES,
  type SessionKind,
  segmentDuration,
  timeByApp,
} from "../lib/activity";
import { formatDuration } from "../lib/timers";

/** A label + big number, the unit every summary card is built from. */
export function Stat({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div className="min-w-0">
      <div className="font-mono text-[10px] uppercase tracking-wider text-white/35">
        {label}
      </div>
      <div className="mt-0.5 font-mono text-[20px] font-semibold tabular-nums leading-none text-white">
        {value}
      </div>
      {hint !== undefined && (
        <div className="mt-1 text-[11px] text-white/40">{hint}</div>
      )}
    </div>
  );
}

export function KindBadge({ kind }: { kind: SessionKind }) {
  const style = KIND_STYLES[kind];
  return (
    <span className="flex shrink-0 items-center gap-1.5 font-mono text-[10px] uppercase tracking-wider text-white/45">
      <span className={`size-1.5 rounded-full ${style.dot}`} />
      {style.label}
    </span>
  );
}

/** Horizontal share bars for the top apps in a range. */
export function AppBreakdown({
  segments,
  now,
  limit = 5,
}: {
  segments: ActivitySegment[];
  now: number;
  limit?: number;
}) {
  const totals = timeByApp(segments, now).slice(0, limit);
  const longest = totals[0]?.ms ?? 0;
  if (totals.length === 0) {
    return (
      <p className="text-[11.5px] text-white/35">Nothing captured yet today.</p>
    );
  }
  return (
    <div className="flex flex-col gap-2">
      {totals.map((entry) => (
        <div key={entry.app} className="min-w-0">
          <div className="flex items-baseline justify-between gap-2">
            <span className="min-w-0 truncate text-[12px] text-white/75">
              {entry.app}
            </span>
            <span className="shrink-0 font-mono text-[11px] tabular-nums text-white/45">
              {formatDuration(entry.ms)}
            </span>
          </div>
          <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-white/5">
            <div
              className="h-full rounded-full bg-accent/70"
              style={{
                // Longest app fills the bar; the rest are relative to it.
                width: `${longest === 0 ? 0 : (entry.ms / longest) * 100}%`,
              }}
            />
          </div>
        </div>
      ))}
    </div>
  );
}

/**
 * Chronological list of segments. Rows become buttons when `onSelect` is
 * given — that is what drives the Sessions review panel.
 */
export function SegmentList({
  segments,
  now,
  onSelect,
  selectedId,
  emptyHint = "No activity captured yet today.",
}: {
  segments: ActivitySegment[];
  now: number;
  onSelect?: (segment: ActivitySegment) => void;
  selectedId?: number | null;
  emptyHint?: string;
}) {
  if (segments.length === 0) {
    return (
      <p className="rounded-lg border border-dashed border-white/10 px-3 py-6 text-center text-[12px] text-white/35">
        {emptyHint}
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-1">
      {segments.map((segment) => {
        const duration = segmentDuration(segment, now);
        const selected = selectedId === segment.id;
        const body = (
          <>
            <span
              className={`size-2 shrink-0 rounded-full ${KIND_STYLES[segment.kind].dot}`}
            />
            <div className="min-w-0 flex-1">
              <div className="flex items-baseline justify-between gap-2">
                <span className="min-w-0 truncate text-[12.5px] font-medium text-white/80">
                  {segment.title.length > 0 ? segment.title : segment.app}
                </span>
                <span className="shrink-0 font-mono text-[11px] tabular-nums text-white/45">
                  {formatDuration(duration)}
                </span>
              </div>
              <div className="flex items-center gap-2">
                <span className="font-mono text-[10.5px] text-white/35">
                  {formatClock(segment.startedAt)}–
                  {segment.endedAt === null
                    ? "now"
                    : formatClock(segment.endedAt)}
                </span>
                {segment.title.length > 0 && (
                  <span className="min-w-0 truncate text-[10.5px] text-white/35">
                    {segment.app}
                  </span>
                )}
                {!segment.reviewed && segment.kind === "activity" && (
                  <span className="shrink-0 font-mono text-[9.5px] uppercase tracking-wider text-amber-300/70">
                    Review
                  </span>
                )}
              </div>
            </div>
          </>
        );

        const className = `flex w-full items-start gap-2.5 rounded-lg border px-2.5 py-2 text-left transition-colors ${
          selected
            ? "border-accent/40 bg-accent-soft"
            : "border-transparent hover:bg-white/5"
        }`;

        return onSelect === undefined ? (
          <div key={segment.id} className={className}>
            {body}
          </div>
        ) : (
          <button
            key={segment.id}
            type="button"
            onClick={() => onSelect(segment)}
            className={className}
          >
            {body}
          </button>
        );
      })}
    </div>
  );
}

/** Scroll region for card bodies, so a long list never stretches the card. */
export function CardBody({ children }: { children: ReactNode }) {
  return <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>;
}
