import type { CSSProperties, ReactNode } from "react";
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

function densityFor(height: number): Density {
  if (height >= 72) return "full";
  if (height >= 26) return "compact";
  if (height >= 12) return "line";
  return "sliver";
}

function blockStyle(state: BlockState, color: string): CSSProperties {
  switch (state) {
    case "pending":
      return {
        backgroundImage: `repeating-linear-gradient(135deg, color-mix(in srgb, ${color} 14%, var(--bg-panel)) 0 6px, color-mix(in srgb, ${color} 7%, var(--bg-panel)) 6px 12px)`,
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
  approved: "border-line bg-panel shadow-xs",
  pending: "border-dashed border-review/60",
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
}

/**
 * One time entry on a Calendar timeline. Height follows duration; the left
 * rail carries the category colour; the state shows as solid (approved),
 * hatched with a dashed amber border (pending), hatched gray (needs you),
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
}: EntryBlockProps) {
  const state = blockState(entry);
  const railColor =
    state === "processing" || state === "needsYou" || !category
      ? "var(--fg-faint)"
      : category.color;
  const confidence = confidenceOf(entry);
  const density = densityFor(height);
  const end =
    state === "building"
      ? Math.max(entry.startedAt, now ?? entry.endedAt)
      : entry.endedAt;
  const duration = formatDuration(end - entry.startedAt);
  const timeText = `${formatTime(entry.startedAt)}–${formatTime(end)} · ${
    state === "building" ? "building" : duration
  }`;
  const description =
    state === "processing"
      ? "Categorizing…"
      : entry.description || "Untitled session";

  const title = (
    <span
      className={`truncate ${state === "processing" ? "font-semibold text-accent" : ""}`}
    >
      {description}
    </span>
  );
  const approvedMark = state === "approved" && (
    <span
      className="shrink-0 text-[11px] text-accent"
      title={`Approved by ${entry.approvedBy ?? "you"}`}
    >
      {!narrow && (entry.approvedBy === "auto" || entry.approvedBy === "rule")
        ? `✓ ${entry.approvedBy}`
        : "✓"}
    </span>
  );
  const status = (
    <span className="ml-auto shrink-0 font-semibold text-[10px]">
      {state === "needsYou" && (
        <span className="text-review">
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
        <span className="text-review">Pending</span>
      )}
      {state === "failed" && (
        <span className="text-danger">Couldn't categorize · Retry</span>
      )}
    </span>
  );
  const chips = state !== "processing" && (
    <>
      {category && (
        <span
          className="inline-flex shrink-0 items-center gap-1 rounded-sm px-1.5 py-0.5 font-medium text-[10px]"
          style={{
            backgroundColor: `color-mix(in srgb, ${category.color} 15%, transparent)`,
            color: category.color,
          }}
        >
          <span
            className="size-1.5 rounded-full"
            style={{ backgroundColor: category.color }}
          />
          {category.name}
        </span>
      )}
      {project && (
        <span className="inline-flex min-w-0 items-center gap-1 rounded-sm bg-surface px-1.5 py-0.5 font-medium text-[10px] text-fg-muted">
          <span
            className="size-1.5 shrink-0 rounded-full"
            style={{ backgroundColor: project.color }}
          />
          <span className="truncate">
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
    <button
      type="button"
      onClick={() => onSelect(entry.id)}
      title={density === "full" && !narrow ? undefined : tooltip}
      aria-label={`${description}, ${timeText}`}
      className={`calendar-entry @container absolute cursor-pointer select-none overflow-hidden border text-left transition-all ${
        narrow ? "right-1 left-0.5" : "right-4 left-0"
      } ${(narrow ? NARROW_DENSITY_CLASSES : DENSITY_CLASSES)[density]} ${
        selected
          ? "z-20 shadow-lg ring-2 ring-accent"
          : "z-10 hover:border-fg-soft/40"
      } ${BLOCK_CLASSES[state]}`}
      style={{
        top: `${top}px`,
        height: `${height}px`,
        borderLeftWidth: narrow ? "3px" : "4px",
        borderLeftStyle: "solid",
        borderLeftColor: railColor,
        ...blockStyle(state, railColor),
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
          duration={state === "building" ? "building" : duration}
          approvedMark={approvedMark}
        />
      ) : (
        <>
          {density === "full" && (
            <div className="flex h-full flex-col justify-between overflow-hidden">
              <div className="min-w-0">
                <div className="flex items-center gap-1.5 font-semibold text-[12px] text-fg-strong">
                  {title}
                  {approvedMark}
                </div>
                <div className="mt-0.5 truncate text-[11px] text-fg-soft">
                  {timeText}
                </div>
              </div>
              <div className="flex items-center gap-1.5 overflow-hidden">
                {chips}
                {status}
              </div>
            </div>
          )}
          {density === "compact" && (
            <div className="flex h-full flex-col justify-center gap-0.5 overflow-hidden">
              <div className="flex items-center gap-1.5 font-semibold text-[11.5px] text-fg-strong">
                {title}
                {approvedMark}
                {height < 40 && status}
              </div>
              {height >= 40 && (
                <div className="flex items-center gap-1.5 overflow-hidden text-[10.5px] text-fg-soft">
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
  return (
    <div className="flex h-full min-w-0 flex-col overflow-hidden text-[10.5px] leading-tight">
      <div className="flex min-w-0 items-center gap-1 font-semibold text-fg-strong">
        <span className="hidden truncate @[8.5rem]:inline">{description}</span>
        <span className="truncate @[8.5rem]:hidden">{categoryName}</span>
        {approvedMark}
      </div>
      {height >= 38 && (
        <div className="truncate text-[10px] text-fg-soft">
          <span className="hidden @[8.5rem]:inline">{categoryName} · </span>
          {duration}
        </div>
      )}
    </div>
  );
}
