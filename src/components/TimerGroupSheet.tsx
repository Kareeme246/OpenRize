import { useState } from "react";
import { FIELD } from "./Page";
import { Field, Sheet } from "./Sheet";

interface TimerGroupSheetProps {
  label: string;
  /** The group the timer is in now, if any. */
  current: string | null;
  /** Every group that exists, offered as one-click choices. */
  groups: string[];
  onSave: (group: string | null) => void;
  onClose: () => void;
}

/** Puts a stopwatch in a group by name, or takes it out of its group. */
export function TimerGroupSheet({
  label,
  current,
  groups,
  onSave,
  onClose,
}: TimerGroupSheetProps) {
  const [draft, setDraft] = useState(current ?? "");
  const name = draft.trim();

  return (
    <Sheet
      title={`Group "${label}"`}
      submitLabel={
        name.length === 0 && current !== null ? "Remove from group" : "Save"
      }
      width="sm"
      canSubmit={name !== (current ?? "")}
      onSubmit={() => {
        onSave(name.length === 0 ? null : name);
        onClose();
      }}
      onClose={onClose}
    >
      <Field
        label="Group"
        htmlFor="timer-group"
        hint="Stopwatches with the same group start, pause and add up together. Leave it empty to remove this one from its group."
      >
        <input
          id="timer-group"
          value={draft}
          placeholder="e.g. Website redesign"
          onChange={(event) => setDraft(event.target.value)}
          className={FIELD}
        />
        {groups.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-1.5">
            {groups.map((group) => (
              <button
                key={group}
                type="button"
                aria-label={`Use group ${group}`}
                onClick={() => setDraft(group)}
                className={`max-w-full truncate rounded-full px-2.5 py-0.5 text-[11.5px] font-medium ${
                  group === name
                    ? "bg-accent/20 text-accent"
                    : "bg-accent/10 text-fg-soft hover:text-accent"
                }`}
              >
                {group}
              </button>
            ))}
          </div>
        )}
      </Field>
    </Sheet>
  );
}
