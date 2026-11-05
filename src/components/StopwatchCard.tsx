import { useCallback, useRef, useState } from "react";
import {
  elapsedMs,
  formatDuration,
  type SegmentEdit,
  type Timer,
  type TimerSection,
  type TimerTag,
} from "../lib/timers";
import type { Category, Project } from "../lib/types";
import { useDismiss } from "../pages/timesheets/useDismiss";
import { AddTimeSheet } from "./AddTimeSheet";
import { ConfirmDialog } from "./ConfirmDialog";
import { HourRing } from "./HourRing";
import { KebabMenu, type MenuEntry } from "./KebabMenu";
import { BUTTON_SECONDARY, FIELD } from "./Page";
import { Field, Sheet } from "./Sheet";
import { TimerInfoSheet } from "./TimerInfoSheet";
import { TimerTagSheet } from "./TimerTagSheet";
import { Tooltip } from "./Tooltip";

interface StopwatchCardProps {
  timer: Timer;
  now: number;
  isFirst?: boolean;
  isLast?: boolean;
  isDragging?: boolean;
  cardRef?: (el: HTMLElement | null) => void;
  onStart: (id: string) => void;
  onPause: (id: string) => void;
  onReset: (id: string) => void;
  /** Resets quietly once "Add to calendar" has logged the time. */
  onClear: (id: string) => void;
  onRename: (id: string, label: string) => void;
  onSetTag: (id: string, tag: string | null) => void;
  /** Every tag in use, with what its linked stopwatches add up to. */
  tags: TimerTag[];
  onStartTag: (tag: string) => void;
  onPauseTag: (tag: string) => void;
  /** Every section, offered by "Move to". */
  sections: TimerSection[];
  onMoveToSection: (id: string, sectionId: string | null) => void;
  /** Adds a section and moves this stopwatch into it. */
  onCreateSection: (name: string, id: string) => void;
  /**
   * Keeps the tag chip's row even without a tag, so rings and times line up
   * across a row of cards when any card on the page is tagged.
   */
  reserveTagRow?: boolean;
  onAddSegment: (id: string, startedAt: number, endedAt: number) => void;
  onUpdateSegment: (id: string, segmentId: number, edit: SegmentEdit) => void;
  onDeleteSegment: (id: string, segmentId: number) => void;
  onSaveState: (id: string) => void;
  onDelete: (id: string) => void;
  onMoveUp?: (id: string) => void;
  onMoveDown?: (id: string) => void;
  onDragStart?: (event: React.DragEvent, id: string) => void;
  onDragEnd?: () => void;
  categories: Category[];
  projects: Project[];
}

export function StopwatchCard({
  timer,
  now,
  isFirst = false,
  isLast = false,
  isDragging = false,
  cardRef,
  onStart,
  onPause,
  onReset,
  onClear,
  onRename,
  onSetTag,
  tags,
  onStartTag,
  onPauseTag,
  sections,
  onMoveToSection,
  onCreateSection,
  reserveTagRow = false,
  onAddSegment,
  onUpdateSegment,
  onDeleteSegment,
  onSaveState,
  onDelete,
  onMoveUp,
  onMoveDown,
  onDragStart,
  onDragEnd,
  categories,
  projects,
}: StopwatchCardProps) {
  const running = timer.startedAt !== null;
  const elapsed = elapsedMs(timer, now);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(timer.label);
  /** Which confirmation is up, if any. Both destroy real accumulated time. */
  const [pending, setPending] = useState<"reset" | "delete" | null>(null);
  const [dialog, setDialog] = useState<
    "info" | "calendar" | "tag" | "section" | null
  >(null);
  /** Frozen when "Add to calendar" opens, so the prefilled range holds still. */
  const [calendarRange, setCalendarRange] = useState<{
    start: number;
    end: number;
  } | null>(null);

  const commitRename = (): void => {
    const next = draft.trim();
    if (next.length > 0 && next !== timer.label) onRename(timer.id, next);
    else setDraft(timer.label);
    setEditing(false);
  };

  const startRename = (): void => {
    setDraft(timer.label);
    setEditing(true);
  };

  const openCalendar = (): void => {
    const end = Date.now();
    const midnight = new Date(end).setHours(0, 0, 0, 0);
    setCalendarRange({ start: Math.max(midnight, end - elapsed), end });
    setDialog("calendar");
  };

  return (
    <section
      ref={cardRef}
      id={`timer-${timer.id}`}
      tabIndex={-1}
      aria-label={`Stopwatch ${timer.label}`}
      draggable={false}
      onDragStart={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if (event.target === event.currentTarget) {
          if (
            (isFirst &&
              (event.key === "ArrowUp" || event.key === "ArrowLeft")) ||
            (isLast &&
              (event.key === "ArrowDown" || event.key === "ArrowRight"))
          ) {
            event.preventDefault();
          }
        }
      }}
      className={`timer-card shape-bleed-table relative flex min-w-0 flex-col items-center gap-3 rounded-xl border border-settings-card-border bg-settings-card p-4 outline-hidden focus:ring-2 focus:ring-accent ${
        running ? "border-l-4 border-l-accent" : ""
      } ${isDragging ? "opacity-50 ring-2 ring-accent shadow-lg" : ""}`}
    >
      <button
        type="button"
        draggable
        aria-label={`Reorder ${timer.label}`}
        title="Drag to reorder"
        onDragStart={(event) => {
          event.stopPropagation();
          onDragStart?.(event, timer.id);
        }}
        onDragEnd={() => {
          onDragEnd?.();
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowUp" || event.key === "ArrowLeft") {
            event.preventDefault();
            if (!isFirst) {
              onMoveUp?.(timer.id);
            }
          } else if (event.key === "ArrowDown" || event.key === "ArrowRight") {
            event.preventDefault();
            if (!isLast) {
              onMoveDown?.(timer.id);
            }
          }
        }}
        className="timer-drag-handle absolute top-2 left-2 grid size-7 place-items-center rounded-md text-fg-faint/60 hover:text-fg hover:bg-surface cursor-move select-none transition-colors"
      >
        <svg
          viewBox="0 0 24 24"
          className="pointer-events-none size-4"
          fill="currentColor"
          aria-hidden="true"
        >
          <circle cx="9" cy="6" r="1.5" />
          <circle cx="15" cy="6" r="1.5" />
          <circle cx="9" cy="12" r="1.5" />
          <circle cx="15" cy="12" r="1.5" />
          <circle cx="9" cy="18" r="1.5" />
          <circle cx="15" cy="18" r="1.5" />
        </svg>
      </button>

      <KebabMenu
        label={`Options for ${timer.label}`}
        className="absolute top-2 right-2"
        entries={[
          { text: "Info", onSelect: () => setDialog("info") },
          { text: "Link...", onSelect: () => setDialog("tag") },
          {
            text: "Move to",
            entries: [
              {
                text: "No section",
                checked: timer.sectionId === null,
                onSelect: () => onMoveToSection(timer.id, null),
              },
              ...(sections.length > 0 ? (["divider"] as MenuEntry[]) : []),
              ...sections.map(
                (section): MenuEntry => ({
                  text: section.name,
                  checked: timer.sectionId === section.id,
                  onSelect: () => onMoveToSection(timer.id, section.id),
                }),
              ),
              "divider",
              { text: "New section...", onSelect: () => setDialog("section") },
            ],
          },
          "divider",
          { text: "Save state", onSelect: () => onSaveState(timer.id) },
          {
            text: "Add to calendar",
            onSelect: openCalendar,
            disabled: elapsed < 60_000,
            hint: "Needs at least a minute of tracked time",
          },
          "divider",
          {
            text: "Move up",
            onSelect: () => onMoveUp?.(timer.id),
            disabled: isFirst,
            hint: "Already first",
          },
          {
            text: "Move down",
            onSelect: () => onMoveDown?.(timer.id),
            disabled: isLast,
            hint: "Already last",
          },
          "divider",
          { text: "Reset to zero", onSelect: () => setPending("reset") },
          {
            text: "Delete",
            onSelect: () => setPending("delete"),
            danger: true,
          },
        ]}
      />

      <div className="flex w-full min-w-0 items-center justify-center gap-1.5 px-6">
        {editing ? (
          <input
            // biome-ignore lint/a11y/noAutofocus: intentional — focuses the rename input the moment editing starts
            autoFocus
            value={draft}
            aria-label="Tracker name"
            onChange={(event) => setDraft(event.target.value)}
            onBlur={commitRename}
            onKeyDown={(event) => {
              if (event.key === "Enter") commitRename();
              if (event.key === "Escape") {
                setDraft(timer.label);
                setEditing(false);
              }
            }}
            className="w-full min-w-0 rounded-md border border-accent/30 bg-inset px-2 py-1 text-center text-[13px] text-fg-strong outline-none"
          />
        ) : (
          <div className="group flex min-w-0 max-w-full items-center justify-center gap-1">
            <Tooltip content="Double-click to rename">
              <button
                type="button"
                onDoubleClick={startRename}
                className="min-w-0 truncate text-center text-[13px] font-semibold text-fg-strong hover:text-accent transition-colors"
              >
                {timer.label}
              </button>
            </Tooltip>
          </div>
        )}
      </div>

      {timer.tag !== null ? (
        <TagChip
          tag={
            tags.find(
              (tag) => tag.name.toLowerCase() === timer.tag?.toLowerCase(),
            ) ?? { name: timer.tag, timers: 1, running: 0, totalMs: elapsed }
          }
          onStart={onStartTag}
          onPause={onPauseTag}
        />
      ) : (
        reserveTagRow && <div aria-hidden="true" className={TAG_ROW} />
      )}

      <HourRing
        elapsed={elapsed}
        running={running}
        onToggle={() => (running ? onPause(timer.id) : onStart(timer.id))}
      />

      <div
        className={`font-mono text-[26px] font-semibold tabular-nums leading-none tracking-tight ${
          running ? "text-accent" : "text-fg-soft"
        }`}
      >
        {formatDuration(elapsed)}
      </div>

      {dialog === "info" && (
        <TimerInfoSheet
          timer={timer}
          total={elapsed}
          runningMs={elapsed - timer.accumulatedMs}
          onRename={(label) => onRename(timer.id, label)}
          onAdd={(startedAt, endedAt) =>
            onAddSegment(timer.id, startedAt, endedAt)
          }
          onUpdate={(segmentId, edit) =>
            onUpdateSegment(timer.id, segmentId, edit)
          }
          onDelete={(segmentId) => onDeleteSegment(timer.id, segmentId)}
          onClose={() => setDialog(null)}
        />
      )}

      {dialog === "tag" && (
        <TimerTagSheet
          label={timer.label}
          current={timer.tag}
          tags={tags.map((tag) => tag.name)}
          onSave={(tag) => onSetTag(timer.id, tag)}
          onClose={() => setDialog(null)}
        />
      )}

      {dialog === "section" && (
        <NewSectionSheet
          label={timer.label}
          onCreate={(name) => onCreateSection(name, timer.id)}
          onClose={() => setDialog(null)}
        />
      )}

      {dialog === "calendar" && calendarRange !== null && (
        <AddTimeSheet
          date={new Date(calendarRange.start)}
          initialStartMs={calendarRange.start}
          initialEndMs={calendarRange.end}
          initialDescription={timer.label}
          categories={categories}
          projects={projects}
          onClose={() => setDialog(null)}
          onCreated={() => onClear(timer.id)}
        />
      )}

      {pending === "reset" && (
        <ConfirmDialog
          title={`Reset "${timer.label}"?`}
          body="Set its time back to 0:00 and stop it."
          confirmLabel="Reset"
          onConfirm={() => {
            setPending(null);
            onReset(timer.id);
          }}
          onCancel={() => setPending(null)}
        />
      )}

      {pending === "delete" && (
        <ConfirmDialog
          title={`Delete "${timer.label}"?`}
          body={`This tracker, its ${formatDuration(elapsed)} and its saved states will be gone.`}
          confirmLabel="Delete"
          onConfirm={() => {
            setPending(null);
            onDelete(timer.id);
          }}
          onCancel={() => setPending(null)}
        />
      )}
    </section>
  );
}

function LinkIcon({ className }: { className: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={className}
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
      <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
    </svg>
  );
}

/** The tag chip's row: one fixed height, whether it holds a chip or not. */
const TAG_ROW = "-mt-1.5 h-5 shrink-0";

interface TagChipProps {
  tag: TimerTag;
  onStart: (tag: string) => void;
  onPause: (tag: string) => void;
}

/** The card's tag, which opens what its linked stopwatches add up to. */
function TagChip({ tag, onStart, onPause }: TagChipProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, ref, close);
  const running = tag.running > 0;

  return (
    <div
      ref={ref}
      className={`relative flex max-w-full justify-center ${TAG_ROW}`}
    >
      <button
        type="button"
        data-timer-tag-badge={tag.name}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={`Linked by ${tag.name}`}
        onClick={() => setOpen(!open)}
        className={`flex h-5 max-w-full items-center gap-1 rounded-full px-2 font-medium text-[11px] text-accent transition-colors ${
          open ? "bg-accent/20" : "bg-accent/10 hover:bg-accent/20"
        }`}
      >
        <LinkIcon className="size-3 shrink-0" />
        <span className="min-w-0 truncate">{tag.name}</span>
      </button>
      {open && (
        <div
          role="dialog"
          aria-label={`Linked by ${tag.name}`}
          data-timer-tag-popover={tag.name}
          className="absolute top-full z-30 mt-1.5 flex w-56 flex-col gap-2 rounded-lg border border-line bg-panel p-3 shadow-black/40 shadow-xl"
        >
          <div className="flex items-baseline justify-between gap-2">
            <span className="flex min-w-0 items-center gap-1.5 font-semibold text-[13px] text-fg-strong">
              <LinkIcon className="size-3 shrink-0 text-accent" />
              <span className="truncate">{tag.name}</span>
            </span>
            <span
              className={`font-mono font-semibold text-[16px] tabular-nums ${
                running ? "text-accent" : "text-fg-soft"
              }`}
            >
              {formatDuration(tag.totalMs)}
            </span>
          </div>
          <div className="text-[11.5px] text-fg-faint">
            {tag.timers} linked · {tag.running} running
          </div>
          <div className="flex gap-1.5">
            <button
              type="button"
              disabled={tag.running === tag.timers}
              onClick={() => onStart(tag.name)}
              className={BUTTON_SECONDARY}
            >
              Start all
            </button>
            <button
              type="button"
              disabled={!running}
              onClick={() => onPause(tag.name)}
              className={BUTTON_SECONDARY}
            >
              Pause all
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

interface NewSectionSheetProps {
  label: string;
  onCreate: (name: string) => void;
  onClose: () => void;
}

/** Names a new section for "Move to", which then holds this stopwatch. */
function NewSectionSheet({ label, onCreate, onClose }: NewSectionSheetProps) {
  const [draft, setDraft] = useState("");
  const name = draft.trim();

  return (
    <Sheet
      title={`Move "${label}" to a new section`}
      submitLabel="Create and move"
      width="sm"
      canSubmit={name.length > 0}
      onSubmit={() => {
        onCreate(name);
        onClose();
      }}
      onClose={onClose}
    >
      <Field label="Section name" htmlFor="timer-new-section">
        <input
          id="timer-new-section"
          value={draft}
          placeholder="e.g. Client calls"
          onChange={(event) => setDraft(event.target.value)}
          className={FIELD}
        />
      </Field>
    </Sheet>
  );
}
