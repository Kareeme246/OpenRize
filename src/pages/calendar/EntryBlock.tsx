import type { CSSProperties, ReactNode } from "react";
import { Tooltip } from "../../components/Tooltip";
import { BAND_LABEL, BAND_TONE, band, percent } from "../../lib/confidence";
import { type BlockState, blockState, confidenceOf } from "../../lib/entries";
import { formatDuration, formatTime } from "../../lib/format";
import type { Category, Project, TimeEntry } from "../../lib/types";

/** How much of a block's content fits its height. */
type Density = "full" | "compact" | "line" | "sliver";

const DENSITY_CLASSES: Record<Density, string> = {
  full: "rounded-lg p-2.5",
  compact: "rounded-lg px-2.5 py-1",
  line: "rounded-md px-2 py-0",
  sliver: "rounded-sm p-0",
};

const NARROW_DENSITY_CLASSES: Record<Density, string> = {
  full: "rounded-md px-1.5 py-1",
  compact: "rounded-md px-1.5 py-0.5",
  line: "rounded-sm px-1 py-0",
  sliver: "rounded-xs p-0",
};

/**
 * Rendered line heights in px (font size x the inherited 1.5), pinned on the
 * text with `leading-[...]` below so the room maths here matches the layout.
 */
const LINE = {
  title: 18,
  titleCompact: 17.25,
  time: 16.5,
  timeCompact: 15.75,
  chipText: 15,
  narrowTitle: 13.125,
  narrowSub: 12.5,
};
/** A chip is one text line plus its py-0.5. */
const CHIP_HEIGHT = LINE.chipText + 4;

/** Vertical padding plus border a density spends inside the block's height. */
const CHROME: Record<Density, number> = {
  full: 22,
  compact: 10,
  line: 2,
  sliver: 0,
};
const NARROW_CHROME: Record<Density, number> = {
  full: 10,
  compact: 6,
  line: 2,
  sliver: 0,
};

/** How many lines each wrappable element of a wide block may take. */
interface LineBudget {
  /** Description lines. */
  title: number;
  /** Text lines inside each category / project chip. */
  chip: number;
  /** Whether chips may flow onto further rows. */
  wrapRow: boolean;
}

interface BudgetInput {
  /** Height the block's content box has. */
  room: number;
  titleLine: number;
  /** Height the content takes with every element on a single line. */
  base: number;
  /** Chips whose text can wrap (category, project). */
  chips: number;
  /** Everything sharing the chip row, which can stack when it wraps. */
  rowItems: number;
  rowLine: number;
  rowGap: number;
}

/**
 * Splits the room left over once every element has one line. The description
 * gets the first spare line, then the chip row, then they take turns; an
 * element only grows by a whole line that fits, so nothing is ever clipped
 * mid-line and whatever cannot grow keeps truncating.
 */
function budgetLines({
  room,
  titleLine,
  base,
  chips,
  rowItems,
  rowLine,
  rowGap,
}: BudgetInput): LineBudget {
  const budget: LineBudget = { title: 1, chip: 1, wrapRow: false };
  let spare = room - base;
  for (let turn = 0; turn < 32; turn++) {
    let grew = false;
    if (spare >= titleLine) {
      budget.title++;
      spare -= titleLine;
      grew = true;
    }
    if (chips > 0) {
      const cost =
        !budget.wrapRow && rowItems > 1
          ? (rowItems - 1) * (rowLine + rowGap)
          : chips * LINE.chipText;
      if (spare >= cost) {
        if (!budget.wrapRow && rowItems > 1) budget.wrapRow = true;
        else budget.chip++;
        spare -= cost;
        grew = true;
      }
    }
    if (!grew) break;
  }
  return budget;
}

/** Wraps to at most `lines` lines, then cuts off with an ellipsis. */
export function clampStyle(lines: number): CSSProperties {
  return {
    display: "-webkit-box",
    WebkitBoxOrient: "vertical",
    WebkitLineClamp: lines,
    overflow: "hidden",
    textOverflow: "ellipsis",
  };
}

function densityFor(height: number): Density {
  if (height >= 72) return "full";
  if (height >= 26) return "compact";
  if (height >= 12) return "line";
  return "sliver";
}

function wideBudget(
  density: Density,
  height: number,
  chips: number,
  hasStatus: boolean,
): LineBudget {
  const room = height - CHROME[density];
  const chipRow = chips > 0 ? CHIP_HEIGHT : LINE.chipText;
  if (density === "full") {
    const rowItems = chips + (hasStatus ? 1 : 0);
    return budgetLines({
      room,
      titleLine: LINE.title,
      base: LINE.title + 2 + LINE.time + (rowItems > 0 ? 6 + chipRow : 0),
      chips,
      rowItems,
      rowLine: chipRow,
      rowGap: 6,
    });
  }
  if (density === "compact" && height >= 40) {
    return budgetLines({
      room,
      titleLine: LINE.titleCompact,
      base: LINE.titleCompact + 2 + Math.max(chipRow, LINE.timeCompact),
      chips,
      rowItems: 1 + chips + (hasStatus ? 1 : 0),
      rowLine: Math.max(chipRow, LINE.timeCompact),
      rowGap: 2,
    });
  }
  const titleLine = density === "compact" ? LINE.titleCompact : LINE.title;
  return budgetLines({
    room,
    titleLine,
    base: titleLine,
    chips: 0,
    rowItems: 0,
    rowLine: 0,
    rowGap: 0,
  });
}

/** Week-column blocks: the first and second text rows share the spare room. */
function narrowBudget(
  density: Density,
  height: number,
): { first: number; second: number } {
  const lines = { first: 1, second: 1 };
  if (density === "line") return lines;
  let spare =
    height -
    NARROW_CHROME[density] -
    LINE.narrowTitle -
    (height >= 38 ? LINE.narrowSub : 0);
  for (let turn = 0; turn < 32; turn++) {
    let grew = false;
    if (spare >= LINE.narrowTitle) {
      lines.first++;
      spare -= LINE.narrowTitle;
      grew = true;
    }
    if (height >= 38 && spare >= LINE.narrowSub) {
      lines.second++;
      spare -= LINE.narrowSub;
      grew = true;
    }
    if (!grew) break;
  }
  return lines;
}

function blockStyle(state: BlockState): CSSProperties {
  switch (state) {
    case "approved":
      return {
        backgroundColor: `color-mix(in srgb, var(--accent) 24%, var(--bg-panel))`,
      };
    case "pending":
      return {
        backgroundImage: `repeating-linear-gradient(135deg, color-mix(in srgb, var(--accent) 14%, var(--bg-panel)) 0 6px, color-mix(in srgb, var(--accent) 7%, var(--bg-panel)) 6px 12px)`,
      };
    case "needsYou":
    case "failed":
      return {
        backgroundImage:
          "repeating-linear-gradient(135deg, var(--bg-surface-1) 0 6px, transparent 6px 12px)",
      };
    default:
      return {};
  }
}

const BLOCK_CLASSES: Record<BlockState, string> = {
  approved: "border-line shadow-xs",
  pending: "border-dashed border-line-strong",
  needsYou: "border-dashed border-line",
  failed: "border-dashed border-danger/50",
  processing: "entry-processing border-line",
  building: "border-dotted border-fg-soft/50 bg-transparent",
};

interface EntryBlockProps {
  entry: TimeEntry;
  top: number;
  height: number;
  selected: boolean;
  category?: Category;
  project?: Project;
  /**
   * Week columns: a smaller block that drops the description when the
   * column is too narrow for it, keeping the category.
   */
  narrow?: boolean;
  onSelect: (id: string) => void;
  now?: number;
  /** The session capturing right now; the now line rides its bottom edge. */
  recording?: boolean;
  column?: { index: number; count: number };
}

/**
 * One time entry on a Calendar timeline. Height follows duration; the block
 * takes the accent colour; the state shows as solid (approved),
 * hatched with a dashed border (pending), hatched gray (needs you),
 * shimmering (categorizing), or dotted (still recording).
 */
export function EntryBlock({
  entry,
  top,
  height,
  selected,
  category,
  project,
  narrow = false,
  onSelect,
  now,
  recording = false,
  column,
}: EntryBlockProps) {
  // The live session reads as recording even while the AI already has a
  // look at it; "Categorizing…" would contradict the now line's tag.
  const state: BlockState = recording ? "building" : blockState(entry);
  const confidence = confidenceOf(entry);
  const density = densityFor(height);
  const end =
    state === "building"
      ? Math.max(entry.startedAt, now ?? entry.endedAt)
      : entry.endedAt;
  const duration = formatDuration(end - entry.startedAt);
  const liveLabel = recording ? "recording" : "building";
  const timeText = `${formatTime(entry.startedAt)}–${formatTime(end)} · ${
    state === "building" ? liveLabel : duration
  }`;
  const description =
    state === "processing"
      ? "Categorizing…"
      : `${entry.source === "agent" ? "Agent · " : ""}${entry.description || "Untitled session"}`;

  const hasStatus =
    state === "needsYou" || state === "pending" || state === "failed";
  const chipCount =
    state === "processing" ? 0 : (category ? 1 : 0) + (project ? 1 : 0);
  const budget = wideBudget(density, height, chipCount, hasStatus);
  const title = (
    <span
      className={`min-w-0 ${state === "processing" ? "font-semibold text-accent" : ""}`}
      style={clampStyle(budget.title)}
    >
      {description}
    </span>
  );
  const approvedMark = state === "approved" && (
    <Tooltip content={`Approved by ${entry.approvedBy ?? "you"}`}>
      <span className="shrink-0 text-[11px] text-accent">
        {!narrow && (entry.approvedBy === "auto" || entry.approvedBy === "rule")
          ? `✓ ${entry.approvedBy}`
          : "✓"}
      </span>
    </Tooltip>
  );
  const status = (
    <span className="ml-auto shrink-0 font-semibold text-[10px]">
      {state === "needsYou" && (
        <span className="text-fg-soft">
          {BAND_LABEL.low}
          {confidence !== undefined && ` · ${percent(confidence)}`}
        </span>
      )}
      {state === "pending" && confidence !== undefined && (
        <span className={`tabular-nums ${BAND_TONE[band(confidence)]}`}>
          {percent(confidence)}
        </span>
      )}
      {state === "pending" && confidence === undefined && (
        <span className="text-fg-soft">Pending</span>
      )}
      {state === "failed" && (
        <span className="text-danger">Couldn't categorize · Retry</span>
      )}
    </span>
  );
  const chipText = clampStyle(budget.chip);
  const chips = state !== "processing" && (
    <>
      {category && (
        <span
          className={`inline-flex items-center gap-1 rounded-sm px-1.5 py-0.5 font-medium text-[10px] leading-[15px] ${
            budget.wrapRow ? "min-w-0 max-w-full" : "max-w-[55%] shrink-0"
          }`}
          style={{
            backgroundColor: `color-mix(in srgb, ${category.color} 15%, transparent)`,
            color: category.color,
          }}
        >
          <span
            className="size-1.5 shrink-0 rounded-full"
            style={{ backgroundColor: category.color }}
          />
          <span className="min-w-0" style={chipText}>
            {category.name}
          </span>
        </span>
      )}
      {project && (
        <span className="inline-flex min-w-0 max-w-full items-center gap-1 rounded-sm bg-surface px-1.5 py-0.5 font-medium text-[10px] text-fg-muted leading-[15px]">
          <span
            className="size-1.5 shrink-0 rounded-full"
            style={{ backgroundColor: project.color }}
          />
          <span className="min-w-0" style={chipText}>
            {project.name}
            {state === "pending" && "?"}
          </span>
        </span>
      )}
    </>
  );

  const tooltip = [
    entry.description,
    timeText,
    [category?.name, project?.name].filter(Boolean).join(" · "),
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <Tooltip content={density === "full" && !narrow ? undefined : tooltip}>
      <button
        type="button"
        onClick={() => onSelect(entry.id)}
        aria-label={`${description}, ${timeText}`}
        className={`calendar-entry @container absolute cursor-pointer select-none overflow-hidden border text-left transition-all ${
          narrow ? "right-1 left-0.5" : "right-4 left-0"
        } ${(narrow ? NARROW_DENSITY_CLASSES : DENSITY_CLASSES)[density]} ${
          selected
            ? "z-20 shadow-lg ring-2 ring-accent"
            : "z-10 hover:border-fg-soft/40"
        } ${recording ? "border-dashed border-danger/45 bg-danger/5" : BLOCK_CLASSES[state]}`}
        style={{
          top: `${top}px`,
          height: `${height}px`,
          ...blockStyle(state),
          ...(column && {
            left: `calc((100% - 16px) * ${column.index} / ${column.count})`,
            width: `calc((100% - 16px) / ${column.count} - 4px)`,
            right: "auto",
          }),
        }}
      >
        {narrow ? (
          <NarrowContent
            density={density}
            height={height}
            description={description}
            categoryName={
              state === "processing"
                ? "Categorizing…"
                : (category?.name ??
                  (state === "approved" ? "Uncategorized" : BAND_LABEL.low))
            }
            duration={state === "building" ? liveLabel : duration}
            approvedMark={approvedMark}
          />
        ) : (
          <>
            {density === "full" && (
              <div className="flex h-full min-w-0 flex-col overflow-hidden">
                <div className="flex items-center gap-1.5 font-semibold text-[12px] text-fg-strong leading-[18px]">
                  {title}
                  {approvedMark}
                </div>
                <div className="mt-0.5 truncate text-[11px] text-fg-soft leading-[16.5px]">
                  {timeText}
                </div>
                <div
                  className={`mt-1.5 flex items-center gap-1.5 overflow-hidden ${
                    budget.wrapRow ? "flex-wrap" : ""
                  }`}
                >
                  {chips}
                  {status}
                </div>
              </div>
            )}
            {density === "compact" && (
              <div className="flex h-full flex-col justify-center gap-0.5 overflow-hidden">
                <div className="flex items-center gap-1.5 font-semibold text-[11.5px] text-fg-strong leading-[17.25px]">
                  {title}
                  {approvedMark}
                  {height < 40 && status}
                </div>
                {height >= 40 && (
                  <div
                    className={`flex items-center gap-x-1.5 gap-y-0.5 overflow-hidden text-[10.5px] text-fg-soft leading-[15.75px] ${
                      budget.wrapRow ? "flex-wrap" : ""
                    }`}
                  >
                    <span className="shrink-0">{timeText}</span>
                    {chips}
                    {status}
                  </div>
                )}
              </div>
            )}
            {density === "line" && (
              <div className="flex h-full items-center gap-1.5 overflow-hidden font-semibold text-[10.5px] text-fg-strong leading-none">
                {title}
                {approvedMark}
                {status}
              </div>
            )}
          </>
        )}
      </button>
    </Tooltip>
  );
}

/**
 * A week-column block: the description when the column has room for it
 * (a container query on the block), otherwise just the category.
 */
function NarrowContent({
  density,
  height,
  description,
  categoryName,
  duration,
  approvedMark,
}: {
  density: Density;
  height: number;
  description: string;
  categoryName: string;
  duration: string;
  approvedMark: ReactNode;
}) {
  if (density === "sliver") return null;
  const lines = narrowBudget(density, height);
  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden text-[10.5px] leading-[13.125px]">
      <div className="flex min-w-0 items-center gap-1 font-semibold text-fg-strong">
        <span className="hidden min-w-0 @[8.5rem]:block">
          <span className="min-w-0" style={clampStyle(lines.first)}>
            {description}
          </span>
        </span>
        <span className="min-w-0 @[8.5rem]:hidden">
          <span className="min-w-0" style={clampStyle(lines.first)}>
            {categoryName}
          </span>
        </span>
        {approvedMark}
      </div>
      {height >= 38 && (
        <div
          className="text-[10px] text-fg-soft leading-[12.5px]"
          style={clampStyle(lines.second)}
        >
          <span className="hidden @[8.5rem]:inline">{categoryName} · </span>
          {duration}
        </div>
      )}
    </div>
  );
}
