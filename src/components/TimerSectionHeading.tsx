import { useState } from "react";
import type { TimerSection } from "../lib/timers";
import { ConfirmDialog } from "./ConfirmDialog";
import { KebabMenu } from "./KebabMenu";
import { Tooltip } from "./Tooltip";

interface TimerSectionHeadingProps {
  section: TimerSection;
  count: number;
  isFirst: boolean;
  isLast: boolean;
  isDropTarget: boolean;
  onRename: (name: string) => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onDelete: () => void;
}

export function TimerSectionHeading({
  section,
  count,
  isFirst,
  isLast,
  isDropTarget,
  onRename,
  onMoveUp,
  onMoveDown,
  onDelete,
}: TimerSectionHeadingProps) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(section.name);
  const [confirming, setConfirming] = useState(false);

  const startRename = (): void => {
    setDraft(section.name);
    setEditing(true);
  };

  const commitRename = (): void => {
    const next = draft.trim();
    if (next.length > 0 && next !== section.name) onRename(next);
    setEditing(false);
  };

  return (
    <div
      data-timer-section-heading={section.name}
      className={`flex min-w-0 items-center gap-2 rounded-lg py-1 transition-colors ${
        isDropTarget ? "bg-accent-soft px-2 ring-1 ring-accent ring-inset" : ""
      }`}
    >
      {editing ? (
        <input
          // biome-ignore lint/a11y/noAutofocus: intentional — focuses the rename input the moment editing starts
          autoFocus
          value={draft}
          aria-label="Section name"
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commitRename}
          onKeyDown={(event) => {
            if (event.key === "Enter") commitRename();
            if (event.key === "Escape") setEditing(false);
          }}
          className="w-64 min-w-0 rounded-md border border-accent/30 bg-inset px-2 py-0.5 font-semibold text-[13px] text-fg-strong outline-none"
        />
      ) : (
        <>
          <h2 className="flex min-w-0">
            <Tooltip content="Double-click to rename">
              <button
                type="button"
                onDoubleClick={startRename}
                className={`min-w-0 truncate font-semibold text-[13px] transition-colors hover:text-accent ${
                  isDropTarget ? "text-accent" : "text-fg-muted"
                }`}
              >
                {section.name}
              </button>
            </Tooltip>
          </h2>
          <span className="shrink-0 text-[11px] text-fg-faint tabular-nums">
            {count}
          </span>
        </>
      )}
      <span aria-hidden="true" className="h-px min-w-4 flex-1 bg-line" />
      <KebabMenu
        label={`Options for section ${section.name}`}
        className="relative shrink-0"
        entries={[
          { text: "Rename", onSelect: startRename },
          "divider",
          {
            text: "Move up",
            onSelect: onMoveUp,
            disabled: isFirst,
            hint: "Already first",
          },
          {
            text: "Move down",
            onSelect: onMoveDown,
            disabled: isLast,
            hint: "Already last",
          },
          "divider",
          {
            text: "Delete section",
            danger: true,
            // Nothing to lose in an empty section, so nothing to confirm.
            onSelect: () => (count === 0 ? onDelete() : setConfirming(true)),
          },
        ]}
      />
      {confirming && (
        <ConfirmDialog
          title={`Delete section "${section.name}"?`}
          body={`Its ${count === 1 ? "stopwatch moves" : `${count} stopwatches move`} to the top of the page, with ${count === 1 ? "its" : "their"} time. Nothing else is deleted.`}
          confirmLabel="Delete section"
          onConfirm={() => {
            setConfirming(false);
            onDelete();
          }}
          onCancel={() => setConfirming(false)}
        />
      )}
    </div>
  );
}
