import {
  type CSSProperties,
  type ReactNode,
  useCallback,
  useRef,
  useState,
} from "react";
import { Dot, EmptyState } from "../../components/Page";
import { Tooltip } from "../../components/Tooltip";
import type { Catalog } from "../../hooks/useCatalog";
import { isSameDay } from "../../lib/dates";
import { isInFlight } from "../../lib/entries";
import { formatDuration, formatTime } from "../../lib/format";
import { formatTargetHours } from "../../lib/settings";
import {
  decimalHours,
  NONE,
  type Sheet,
  type SheetCell,
  type SheetRow,
  type SheetRows,
} from "../../lib/timesheetGrid";
import type { TimeEntry } from "../../lib/types";
import { useDismiss } from "./useDismiss";

/** Weekend columns are struck through with Rise's diagonal hatch. */
const HATCH: CSSProperties = {
  backgroundImage:
    "repeating-linear-gradient(135deg, var(--line) 0 1px, transparent 1px 9px)",
};
/** The current day's column carries a faint accent wash top to bottom. */
const TODAY: CSSProperties = {
  backgroundColor: "color-mix(in srgb, var(--accent) 7%, transparent)",
};
const FILLED: CSSProperties = {
  backgroundColor: "color-mix(in srgb, var(--accent) 30%, var(--bg-panel))",
};

interface Column {
  start: number;
  weekend: boolean;
  today: boolean;
}

interface Named {
  label: string;
  color: string;
}

const ROW_OPTIONS: { value: SheetRows; label: string }[] = [
  { value: "project", label: "Project" },
  { value: "category", label: "Category" },
];

export interface SheetGridProps {
  sheet: Sheet;
  edges: number[];
  scale: "day" | "week";
  rows: SheetRows;
  onRowsChange: (rows: SheetRows) => void;
  catalog: Catalog;
  /** The scale's expected hours, the denominator of every progress line. */
  targetMs: number;
  emptyTitle: string;
  emptyHint?: string;
  emptyAction?: ReactNode;
  /** Week: a day cell zooms into that day. */
  onOpenDay: (dayStart: number) => void;
  /** A review badge opens My Timesheet's review queue for the range. */
  onReview: () => void;
  /** Day: an expanded row's entries can be accepted in place. */
  onApprove: (entry: TimeEntry) => void;
}

function columnStyle(column: Column): CSSProperties | undefined {
  if (column.today) return TODAY;
  if (column.weekend) return HATCH;
  return undefined;
}

/** `+5.4`: the part of a cell still waiting on review. */
function Delta({ ms, inset = "right-1.5" }: { ms: number; inset?: string }) {
  if (ms <= 0) return null;
  return (
    <span
      className={`absolute top-1 ${inset} font-semibold text-[10px] text-review tabular-nums`}
    >
      +{decimalHours(ms)}
    </span>
  );
}

function ReviewBadge({
  cell,
  onReview,
}: {
  cell: SheetCell;
  onReview: () => void;
}) {
  if (cell.review === 0) return null;
  return (
    <Tooltip
      content={`${formatDuration(cell.reviewMs)} across ${cell.review} entries waiting on review`}
    >
      <button
        type="button"
        onClick={onReview}
        className="flex h-6 shrink-0 items-center gap-1 rounded-md border border-review/40 bg-review/10 pr-1 pl-1.5 font-semibold text-[11px] text-review transition-colors hover:bg-review/20"
      >
        +{decimalHours(cell.reviewMs)} Review
        <span className="min-w-4 rounded-full bg-review px-1 text-center text-[10px] text-canvas tabular-nums leading-4">
          {cell.review}
        </span>
      </button>
    </Tooltip>
  );
}

/** `4.6h / 40h` under a row's name, as Rise draws a member's capacity. */
function ProgressLine({
  ms,
  targetMs,
  color,
}: {
  ms: number;
  targetMs: number;
  color: string;
}) {
  const share = targetMs > 0 ? Math.min(1, ms / targetMs) : 0;
  return (
    <div className="mt-1.5 flex items-center gap-2">
      <div
        role="progressbar"
        aria-label="Share of expected hours"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(share * 100)}
        className="h-1 min-w-0 flex-1 overflow-hidden rounded-full bg-surface-strong"
      >
        <div
          className="h-full rounded-full"
          style={{ width: `${share * 100}%`, backgroundColor: color }}
        />
      </div>
      <span className="shrink-0 text-[10.5px] text-fg-faint tabular-nums">
        {decimalHours(ms) || "0"}h / {formatTargetHours(targetMs)}
      </span>
    </div>
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={`size-3 shrink-0 text-fg-faint transition-transform ${open ? "rotate-90" : ""}`}
      fill="none"
      stroke="currentColor"
      strokeWidth={2.2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="m9 6 6 6-6 6" />
    </svg>
  );
}

/** The header cell's row picker, Rise's `TEAM MEMBER ⌄`. */
function RowsMenu({
  value,
  onChange,
}: {
  value: SheetRows;
  onChange: (rows: SheetRows) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, ref, close);
  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label="Rows"
        onClick={() => setOpen(!open)}
        className="flex w-full items-center justify-between gap-2 rounded-md py-1 font-semibold text-[11px] text-fg-soft uppercase tracking-wider hover:text-fg"
      >
        {value}
        <svg
          viewBox="0 0 24 24"
          className={`size-3.5 transition-transform ${open ? "rotate-180" : ""}`}
          fill="none"
          stroke="currentColor"
          strokeWidth={2}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      {open && (
        <div
          role="menu"
          className="absolute top-full left-0 z-30 mt-1 w-44 rounded-lg border border-line bg-panel p-1 shadow-black/40 shadow-xl"
        >
          {ROW_OPTIONS.map((option) => (
            <button
              key={option.value}
              type="button"
              role="menuitemradio"
              aria-checked={option.value === value}
              onClick={() => {
                onChange(option.value);
                setOpen(false);
              }}
              className={`flex w-full items-center justify-between rounded-md px-2.5 py-1.5 text-left text-[12px] normal-case tracking-normal ${
                option.value === value
                  ? "bg-surface font-medium text-accent"
                  : "text-fg-muted hover:bg-surface hover:text-fg"
              }`}
            >
              {option.label}
              {option.value === value && <span aria-hidden="true">✓</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function EntryAction({
  entry,
  onApprove,
  onReview,
}: {
  entry: TimeEntry;
  onApprove: (entry: TimeEntry) => void;
  onReview: () => void;
}) {
  if (entry.status === "approved") {
    return <span className="text-[11.5px] text-fg-faint">Approved</span>;
  }
  if (isInFlight(entry)) {
    return <span className="text-[11.5px] text-fg-faint">Processing</span>;
  }
  if (!entry.categoryId) {
    return (
      <button
        type="button"
        onClick={onReview}
        className="font-semibold text-[11.5px] text-review hover:underline"
      >
        Review
      </button>
    );
  }
  return (
    <button
      type="button"
      onClick={() => onApprove(entry)}
      className="flex items-center gap-1 font-semibold text-[11.5px] text-success hover:underline"
    >
      <svg
        viewBox="0 0 24 24"
        className="size-3.5"
        fill="none"
        stroke="currentColor"
        strokeWidth={2.4}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d="M20 6 9 17l-5-5" />
      </svg>
      Accept
    </button>
  );
}

/**
 * The Analyze > Timesheets matrix (Rise's Timesheets week grid): named rows
 * with a progress line, one column per day with the current day lit and
 * weekends hatched, a total column carrying the review badge, and a footer
 * total row. An expanded row breaks down by the other dimension in Week, and
 * lists its entries in Day.
 */
export function SheetGrid({
  sheet,
  edges,
  scale,
  rows,
  onRowsChange,
  catalog,
  targetMs,
  emptyTitle,
  emptyHint,
  emptyAction,
  onOpenDay,
  onReview,
  onApprove,
}: SheetGridProps) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const now = new Date();
  const columns: Column[] = edges.slice(0, -1).map((start) => {
    const day = new Date(start);
    return {
      start,
      weekend: day.getDay() === 0 || day.getDay() === 6,
      today: isSameDay(day, now),
    };
  });
  const width = columns.length + 2;

  const toggle = (key: string): void =>
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const nameOf = (dimension: SheetRows, key: string): Named => {
    if (dimension === "project") {
      const project = catalog.projectById.get(key);
      return project
        ? { label: project.name, color: project.color }
        : {
            label: key === NONE ? "No project" : "Unknown project",
            color: "var(--fg-ghost)",
          };
    }
    const category = catalog.categoryById.get(key);
    return category
      ? { label: category.name, color: category.color }
      : {
          label: key === NONE ? "Uncategorized" : "Unknown category",
          color: "var(--fg-ghost)",
        };
  };
  const other: SheetRows = rows === "project" ? "category" : "project";

  const renderRow = (row: SheetRow, groupKey: string) => {
    const rowKey = `${groupKey}/${row.key}`;
    const open = expanded.has(rowKey);
    const name = nameOf(rows, row.key);
    return [
      <tr key={rowKey} className="border-line border-t">
        <th scope="row" className="px-4 py-2.5 text-left font-normal">
          <button
            type="button"
            onClick={() => toggle(rowKey)}
            aria-expanded={open}
            aria-label={`${name.label}, ${decimalHours(row.total.ms) || "0"} of ${formatTargetHours(targetMs)}`}
            className="flex w-full min-w-0 items-center gap-2.5 text-left"
          >
            <Chevron open={open} />
            <Dot color={name.color} size={10} />
            <span className="min-w-0 flex-1">
              <span className="block truncate font-medium text-[13px] text-fg-strong">
                {name.label}
              </span>
              <ProgressLine
                ms={row.total.ms}
                targetMs={targetMs}
                color={name.color}
              />
            </span>
          </button>
        </th>
        {row.days.map((cell, day) => {
          const column = columns[day];
          const label = decimalHours(cell.ms);
          return (
            <td
              key={column.start}
              className="px-1 py-2"
              style={columnStyle(column)}
            >
              {cell.ms > 0 ? (
                <Tooltip
                  content={`${formatDuration(cell.ms)}${
                    cell.reviewMs > 0
                      ? ` · ${formatDuration(cell.reviewMs)} to review`
                      : ""
                  }`}
                >
                  <button
                    type="button"
                    onClick={() =>
                      scale === "week"
                        ? onOpenDay(column.start)
                        : toggle(rowKey)
                    }
                    aria-label={`${name.label}, ${new Date(column.start).toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}: ${formatDuration(cell.ms)}`}
                    className="relative flex h-12 w-full items-center justify-center rounded-md font-semibold text-[14px] text-fg-strong tabular-nums outline-none transition-[filter] hover:brightness-110 focus-visible:ring-2 focus-visible:ring-accent"
                    style={FILLED}
                  >
                    {label}
                    <Delta ms={cell.reviewMs} />
                  </button>
                </Tooltip>
              ) : (
                <div
                  className={`h-12 rounded-md ${column.weekend ? "" : "bg-surface"}`}
                />
              )}
            </td>
          );
        })}
        <td className="px-4 py-2">
          <div className="flex items-center justify-end gap-2.5">
            <span className="font-semibold text-[14px] text-fg-strong tabular-nums">
              {decimalHours(row.total.ms)}
            </span>
            <ReviewBadge cell={row.total} onReview={onReview} />
          </div>
        </td>
      </tr>,
      ...(open
        ? scale === "week"
          ? row.children.map((child) => {
              const childName = nameOf(other, child.key);
              return (
                <tr key={`${rowKey}/${child.key}`} className="bg-inset-soft">
                  <th
                    scope="row"
                    className="py-1.5 pr-4 pl-[2.6rem] text-left font-normal"
                  >
                    <span className="flex min-w-0 items-center gap-2 text-[12px] text-fg-muted">
                      <Dot color={childName.color} />
                      <span className="truncate">{childName.label}</span>
                    </span>
                  </th>
                  {child.days.map((cell, day) => (
                    <td
                      key={columns[day].start}
                      className="relative px-1 py-1.5 text-center text-[12px] text-fg-muted tabular-nums"
                      style={columnStyle(columns[day])}
                    >
                      {decimalHours(cell.ms) || (
                        <span className="text-fg-ghost">–</span>
                      )}
                    </td>
                  ))}
                  <td className="px-4 py-1.5 text-right text-[12px] text-fg-muted tabular-nums">
                    {decimalHours(child.total.ms)}
                  </td>
                </tr>
              );
            })
          : row.entries.map((entry) => {
              const tag =
                entry[other === "project" ? "projectId" : "categoryId"];
              const tagName = nameOf(other, tag ?? NONE);
              return (
                <tr key={`${rowKey}/${entry.id}`} className="bg-inset-soft">
                  <td colSpan={width} className="py-2 pr-4 pl-[2.6rem]">
                    <div className="grid grid-cols-[5.5rem_minmax(0,1fr)_minmax(0,11rem)_4.5rem_5.5rem] items-center gap-4 text-[12px]">
                      <span className="text-fg-soft tabular-nums">
                        {formatTime(entry.startedAt)}
                      </span>
                      <span className="line-clamp-2 text-fg">
                        {entry.description || "Untitled entry"}
                      </span>
                      <span className="flex min-w-0 items-center gap-1.5 justify-self-start rounded-full border border-line bg-surface px-2 py-0.5 text-[11.5px] text-fg-muted">
                        <Dot color={tagName.color} />
                        <span className="truncate">{tagName.label}</span>
                      </span>
                      <span className="text-right text-fg-muted tabular-nums">
                        {formatDuration(entry.endedAt - entry.startedAt)}
                      </span>
                      <span className="flex justify-end">
                        <EntryAction
                          entry={entry}
                          onApprove={onApprove}
                          onReview={onReview}
                        />
                      </span>
                    </div>
                  </td>
                </tr>
              );
            })
        : []),
    ];
  };

  return (
    <div className="shape-bleed-table overflow-x-auto rounded-xl border border-line bg-panel">
      <table
        className={`w-full table-fixed border-collapse ${scale === "week" ? "min-w-[860px]" : "min-w-[640px]"}`}
      >
        <colgroup>
          <col className="w-[17rem]" />
          {columns.map((column) => (
            <col key={column.start} />
          ))}
          <col className={scale === "week" ? "w-[13rem]" : "w-[16rem]"} />
        </colgroup>
        <thead>
          <tr>
            <th scope="col" className="px-4 py-3 text-left align-middle">
              <RowsMenu value={rows} onChange={onRowsChange} />
            </th>
            {columns.map((column, index) => {
              const day = new Date(column.start);
              const showMonth = index === 0 || day.getDate() === 1;
              return (
                <th
                  key={column.start}
                  scope="col"
                  className="px-1 py-3 text-center"
                  style={columnStyle(column)}
                >
                  <div
                    className={`font-medium text-[11px] uppercase tracking-wider ${
                      column.today ? "text-accent" : "text-fg-soft"
                    }`}
                  >
                    {day.toLocaleDateString(undefined, {
                      weekday: scale === "day" ? "long" : "short",
                    })}
                  </div>
                  <div className="mt-1.5 flex h-7 items-center justify-center gap-1.5">
                    {showMonth && (
                      <span className="font-medium text-[10.5px] text-fg-faint uppercase">
                        {day.toLocaleDateString(undefined, { month: "short" })}
                      </span>
                    )}
                    <span
                      className={`flex h-7 min-w-7 items-center justify-center rounded-full font-semibold text-[15px] tabular-nums ${
                        column.today
                          ? "bg-accent px-1 text-accent-fg"
                          : "text-fg-strong"
                      }`}
                    >
                      {day.getDate()}
                    </span>
                  </div>
                </th>
              );
            })}
            <th
              scope="col"
              className="px-4 py-3 text-right font-medium text-[11px] text-fg-soft uppercase tracking-wider"
            >
              Total <span className="normal-case">(hours)</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {sheet.groups.length === 0 ? (
            <tr className="border-line border-t">
              <td colSpan={width}>
                <EmptyState
                  title={emptyTitle}
                  hint={emptyHint}
                  action={emptyAction}
                />
              </td>
            </tr>
          ) : (
            sheet.groups.flatMap((group) => [
              ...(group.key !== "all"
                ? [
                    <tr
                      key={`group/${group.key}`}
                      className="border-line border-t bg-inset-soft"
                    >
                      <th
                        scope="rowgroup"
                        className="px-4 py-2 text-left font-semibold text-[10.5px] text-fg-soft uppercase tracking-wider"
                      >
                        {group.key === NONE
                          ? "No client"
                          : (catalog.clientById.get(group.key)?.name ??
                            "Unknown client")}
                      </th>
                      {group.days.map((cell, day) => (
                        <td
                          key={columns[day].start}
                          className="px-1 py-2 text-center text-[11.5px] text-fg-soft tabular-nums"
                          style={columnStyle(columns[day])}
                        >
                          {decimalHours(cell.ms)}
                        </td>
                      ))}
                      <td className="px-4 py-2 text-right text-[11.5px] text-fg-soft tabular-nums">
                        {decimalHours(group.total.ms)}
                      </td>
                    </tr>,
                  ]
                : []),
              ...group.rows.flatMap((row) => renderRow(row, group.key)),
            ])
          )}
        </tbody>
        <tfoot>
          <tr className="border-line border-t">
            <th scope="row" className="px-4 py-4 text-left">
              <span className="block font-semibold text-[11px] text-fg-muted uppercase tracking-wider">
                Total <span className="normal-case">(hours)</span>
              </span>
              <ProgressLine
                ms={sheet.total.ms}
                targetMs={targetMs}
                color="var(--accent)"
              />
            </th>
            {sheet.days.map((cell, day) => (
              <td
                key={columns[day].start}
                className="relative px-1 py-4 text-center font-semibold text-[14px] text-fg-strong tabular-nums"
                style={columnStyle(columns[day])}
              >
                {decimalHours(cell.ms) || (
                  <span className="font-normal text-fg-ghost">–</span>
                )}
                <Delta ms={cell.reviewMs} />
              </td>
            ))}
            <td className="relative px-4 py-4 text-right font-semibold text-[14px] text-fg-strong tabular-nums">
              {decimalHours(sheet.total.ms) || (
                <span className="font-normal text-fg-ghost">–</span>
              )}
              <Delta ms={sheet.total.reviewMs} inset="right-4" />
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
}
