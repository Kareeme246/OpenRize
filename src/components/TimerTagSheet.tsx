import { useState } from "react";
import { FIELD } from "./Page";
import { Field, Sheet } from "./Sheet";

interface TimerTagSheetProps {
  label: string;
  /** The timer's tag now, if any. */
  current: string | null;
  /** Every tag in use, offered as one-click choices. */
  tags: string[];
  onSave: (tag: string | null) => void;
  onClose: () => void;
}

/** Links a stopwatch to others by giving it their tag, or unlinks it. */
export function TimerTagSheet({
  label,
  current,
  tags,
  onSave,
  onClose,
}: TimerTagSheetProps) {
  const [draft, setDraft] = useState(current ?? "");
  const name = draft.trim();

  return (
    <Sheet
      title={`Link "${label}"`}
      submitLabel={name.length === 0 && current !== null ? "Unlink" : "Save"}
      width="sm"
      canSubmit={name !== (current ?? "")}
      onSubmit={() => {
        onSave(name.length === 0 ? null : name);
        onClose();
      }}
      onClose={onClose}
    >
      <Field
        label="Tag"
        htmlFor="timer-tag"
        hint="Stopwatches with the same tag are linked: starting or pausing one starts or pauses them all. Leave it empty to unlink."
      >
        <input
          id="timer-tag"
          value={draft}
          placeholder="e.g. Website redesign"
          onChange={(event) => setDraft(event.target.value)}
          className={FIELD}
        />
        {tags.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1.5">
            {tags.map((tag) => (
              <button
                key={tag}
                type="button"
                aria-label={`Use tag ${tag}`}
                onClick={() => setDraft(tag)}
                className={`max-w-full truncate rounded-full px-2.5 py-0.5 text-[11.5px] font-medium ${
                  tag === name
                    ? "bg-accent/20 text-accent"
                    : "bg-accent/10 text-fg-soft hover:text-accent"
                }`}
              >
                {tag}
              </button>
            ))}
          </div>
        )}
      </Field>
    </Sheet>
  );
}
