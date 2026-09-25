import { type ReactNode, useState } from "react";
import { formatDuration, formatHours } from "../lib/format";

/**
 * Small SVG charts for totals by category, project, or app. Identity is
 * never colour alone: category colours are user data, so every chart ships a
 * named legend, a 2px surface gap between touching marks, and a hover
 * tooltip, and each view that uses them also offers the numbers as a table.
 */

export interface Slice {
  key: string;
  label: string;
  color: string;
  ms: number;
}

const GAP_PX = 2;

/** A donut with a legend beside it: share of time by group. */
export function Donut({
  slices,
  size = 132,
  title,
  emptyLabel = "No time yet",
  maxLegend = 6,
  stacked = false,
}: {
  slices: Slice[];
  size?: number;
  title: string;
  emptyLabel?: string;
  /** Past this many, the smallest groups fold into "Other". */
  maxLegend?: number;
  /** Ring above the legend, for narrow cards side by side. */
  stacked?: boolean;
}) {
  const [hovered, setHovered] = useState<string | null>(null);
  const total = slices.reduce((sum, slice) => sum + slice.ms, 0);
  const sorted = [...slices]
    .filter((s) => s.ms > 0)
    .sort((a, b) => b.ms - a.ms);
  // Folding a single group into "Other" would only hide its name.
  const keep = sorted.length > maxLegend + 1 ? maxLegend : sorted.length;
  const shown = sorted.slice(0, keep);
  const rest = sorted.slice(keep);
  if (rest.length > 0) {
    shown.push({
      key: "__other",
      label: `Other (${rest.length})`,
      color: "var(--fg-ghost)",
      ms: rest.reduce((sum, slice) => sum + slice.ms, 0),
    });
  }

  const stroke = 14;
  const radius = (size - stroke) / 2;
  const circumference = 2 * Math.PI * radius;
  // One slice fills the ring; otherwise each leaves a surface gap.
  const gap = shown.length > 1 ? GAP_PX : 0;
  let offset = 0;
  const focus = shown.find((slice) => slice.key === hovered);

  return (
    <figure
      className={`flex min-w-0 gap-4 ${stacked ? "flex-col" : "items-center"}`}
    >
      <svg
        viewBox={`0 0 ${size} ${size}`}
        width={size}
        height={size}
        role="img"
        aria-label={`${title}: ${shown
          .map((s) => `${s.label} ${formatDuration(s.ms)}`)
          .join(", ")}`}
        className={`shrink-0 ${stacked ? "self-center" : ""}`}
      >
        <circle
          cx={size / 2}
          cy={size / 2}
          r={radius}
          fill="none"
          stroke="var(--bg-surface-2)"
          strokeWidth={stroke}
        />
        {total > 0 &&
          shown.map((slice) => {
            const length = (slice.ms / total) * circumference;
            const dash = Math.max(0, length - gap);
            const element = (
              // biome-ignore lint/a11y/noStaticElementInteractions: hover only mirrors the legend, which carries the same values as text.
              <circle
                key={slice.key}
                cx={size / 2}
                cy={size / 2}
                r={radius}
                fill="none"
                stroke={slice.color}
                strokeWidth={hovered === slice.key ? stroke + 3 : stroke}
                strokeDasharray={`${dash} ${circumference - dash}`}
                strokeDashoffset={-offset}
                transform={`rotate(-90 ${size / 2} ${size / 2})`}
                onMouseEnter={() => setHovered(slice.key)}
                onMouseLeave={() => setHovered(null)}
                className="transition-[stroke-width]"
              >
                <title>{`${slice.label}: ${formatDuration(slice.ms)} (${Math.round((slice.ms / total) * 100)}%)`}</title>
              </circle>
            );
            offset += length;
            return element;
          })}
        <text
          x="50%"
          y="47%"
          textAnchor="middle"
          className="fill-fg-strong font-semibold text-[15px]"
        >
          {total > 0 ? formatDuration(focus?.ms ?? total) : "–"}
        </text>
        <text
          x="50%"
          y="61%"
          textAnchor="middle"
          className="fill-fg-soft text-[10px]"
        >
          {focus
            ? `${Math.round((focus.ms / total) * 100)}%`
            : total > 0
              ? "total"
              : emptyLabel}
        </text>
      </svg>
      {/* Capped so a wide card keeps each value near its label. */}
      <figcaption className="min-w-0 max-w-sm flex-1 space-y-1 text-[11.5px]">
        <div className="mb-1.5 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
          {title}
        </div>
        {shown.length === 0 && (
          <div className="text-fg-faint">{emptyLabel}</div>
        )}
        {shown.map((slice) => (
          // biome-ignore lint/a11y/noStaticElementInteractions: hover highlights the matching ring slice; the row is plain text.
          <div
            key={slice.key}
            onMouseEnter={() => setHovered(slice.key)}
            onMouseLeave={() => setHovered(null)}
            className={`flex items-center gap-2 rounded px-1 ${
              hovered === slice.key ? "bg-surface" : ""
            }`}
          >
            <span
              className="size-2 shrink-0 rounded-full"
              style={{ backgroundColor: slice.color }}
            />
            <span className="min-w-0 flex-1 truncate text-fg-muted">
              {slice.label}
            </span>
            <span className="shrink-0 font-mono text-fg-soft tabular-nums">
              {formatDuration(slice.ms)}
            </span>
          </div>
        ))}
      </figcaption>
    </figure>
  );
}

/** A thin horizontal bar split by group, for a month cell or a row. */
export function StackedBar({
  slices,
  totalMs,
  height = 6,
}: {
  slices: Slice[];
  /** The bar's full width in ms; defaults to the slices' sum. */
  totalMs?: number;
  height?: number;
}) {
  const sum = slices.reduce((acc, slice) => acc + slice.ms, 0);
  const total = Math.max(totalMs ?? sum, sum);
  if (total <= 0) return null;
  return (
    <div
      className="flex w-full overflow-hidden rounded-full bg-surface-strong"
      style={{ height, gap: GAP_PX }}
      aria-hidden="true"
    >
      {slices
        .filter((slice) => slice.ms > 0)
        .map((slice) => (
          <span
            key={slice.key}
            title={`${slice.label}: ${formatDuration(slice.ms)}`}
            className="block h-full first:rounded-l-full last:rounded-r-full"
            style={{
              width: `${(slice.ms / total) * 100}%`,
              backgroundColor: slice.color,
            }}
          />
        ))}
    </div>
  );
}

export interface Column {
  key: string;
  label: string;
  slices: Slice[];
}

/** Clean hour ticks for a y-axis: 0, 2h, 4h, ... */
function hourTicks(maxMs: number): number[] {
  const maxHours = Math.max(1, maxMs / 3_600_000);
  const steps = [0.5, 1, 2, 4, 5, 10, 20, 25, 50, 100, 200, 500];
  const step = steps.find((candidate) => maxHours / candidate <= 4) ?? 1000;
  const ticks: number[] = [];
  for (let hours = 0; hours <= maxHours + step * 0.001; hours += step) {
    ticks.push(hours * 3_600_000);
  }
  if (ticks[ticks.length - 1] < maxMs) {
    ticks.push(ticks[ticks.length - 1] + step * 3_600_000);
  }
  return ticks;
}

/** Stacked columns, one per bucket (daily time by category). */
export function StackedColumns({
  columns,
  legend,
  title,
  height = 180,
}: {
  columns: Column[];
  /** Every group that appears, in the chart's fixed colour order. */
  legend: Slice[];
  title: string;
  height?: number;
}) {
  const [tip, setTip] = useState<{
    column: Column;
    slice: Slice;
    x: number;
  } | null>(null);
  const totals = columns.map((column) =>
    column.slices.reduce((sum, slice) => sum + slice.ms, 0),
  );
  const ticks = hourTicks(Math.max(0, ...totals));
  const top = ticks[ticks.length - 1] || 1;
  const labelEvery = Math.max(1, Math.ceil(columns.length / 16));

  return (
    <figure className="min-w-0">
      <div className="mb-2 flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
          {title}
        </span>
        {legend.length > 1 &&
          legend.map((slice) => (
            <span
              key={slice.key}
              className="inline-flex items-center gap-1.5 text-[11px] text-fg-muted"
            >
              <span
                className="size-2 rounded-full"
                style={{ backgroundColor: slice.color }}
              />
              {slice.label}
            </span>
          ))}
      </div>
      <div className="relative flex gap-2" style={{ height }}>
        <div className="relative w-8 shrink-0 font-mono text-[10px] text-fg-faint">
          {ticks.map((tick) => (
            <span
              key={tick}
              className="absolute right-0 translate-y-1/2"
              style={{ bottom: `${(tick / top) * 100}%` }}
            >
              {formatHours(tick, "0")}
            </span>
          ))}
        </div>
        <div className="relative min-w-0 flex-1">
          {ticks.map((tick) => (
            <div
              key={tick}
              className="pointer-events-none absolute right-0 left-0 border-line-soft border-t"
              style={{ bottom: `${(tick / top) * 100}%` }}
            />
          ))}
          <div className="absolute inset-0 flex items-end justify-around">
            {columns.map((column, index) => (
              <div
                key={column.key}
                className="flex h-full min-w-0 flex-1 flex-col-reverse items-center"
              >
                <div
                  className="flex w-full max-w-6 flex-col-reverse overflow-hidden rounded-t"
                  style={{
                    height: `${(totals[index] / top) * 100}%`,
                    gap: totals[index] > 0 ? GAP_PX : 0,
                  }}
                >
                  {column.slices
                    .filter((slice) => slice.ms > 0)
                    .map((slice) => (
                      // biome-ignore lint/a11y/noStaticElementInteractions: the tooltip repeats what the table view lists.
                      <span
                        key={slice.key}
                        onMouseEnter={(event) =>
                          setTip({
                            column,
                            slice,
                            x:
                              event.currentTarget.getBoundingClientRect().left -
                              (event.currentTarget
                                .closest("figure")
                                ?.getBoundingClientRect().left ?? 0),
                          })
                        }
                        onMouseLeave={() => setTip(null)}
                        className="block w-full shrink-0"
                        style={{
                          flexGrow: slice.ms,
                          flexBasis: 0,
                          backgroundColor: slice.color,
                        }}
                      />
                    ))}
                </div>
              </div>
            ))}
          </div>
        </div>
        {tip && (
          <div
            role="tooltip"
            className="pointer-events-none absolute top-0 z-10 rounded-md border border-line bg-panel px-2 py-1 text-[11px] shadow-lg"
            style={{ left: Math.max(0, tip.x - 20) }}
          >
            <div className="font-semibold text-fg-strong">
              {tip.column.label}
            </div>
            <div className="flex items-center gap-1.5 text-fg-muted">
              <span
                className="size-2 rounded-full"
                style={{ backgroundColor: tip.slice.color }}
              />
              {tip.slice.label} · {formatDuration(tip.slice.ms)}
            </div>
          </div>
        )}
      </div>
      <div className="mt-1 flex gap-2">
        <div className="w-8 shrink-0" />
        <div className="flex min-w-0 flex-1 justify-around">
          {columns.map((column, index) => (
            <span
              key={column.key}
              className="min-w-0 flex-1 truncate text-center text-[10px] text-fg-faint"
            >
              {index % labelEvery === 0 ? column.label : ""}
            </span>
          ))}
        </div>
      </div>
    </figure>
  );
}

/** A labelled bar for ranked lists ("top apps"). */
export function BarRow({
  label,
  ms,
  maxMs,
  color = "var(--accent)",
  trailing,
}: {
  label: ReactNode;
  ms: number;
  maxMs: number;
  color?: string;
  trailing?: ReactNode;
}) {
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between gap-2 text-[11.5px]">
        <span className="min-w-0 truncate text-fg-muted">{label}</span>
        <span className="shrink-0 font-mono text-fg-soft tabular-nums">
          {trailing ?? formatDuration(ms)}
        </span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-surface">
        <div
          className="h-full rounded-full"
          style={{
            width: `${maxMs > 0 ? (ms / maxMs) * 100 : 0}%`,
            backgroundColor: color,
          }}
        />
      </div>
    </div>
  );
}
