import { useId, useState } from "react";
import { FIELD } from "./Page";
import { Field, Sheet } from "./Sheet";

interface TimerGroupSheetProps {
  label: string;
  /** The group the timer is in now, if any. */
  current: string | null;
  /** Every group that exists, offered as suggestions. */
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
  const listId = useId();
  const name = draft.trim();

  return (
    <Sheet
      title={`Group "${label}"`}
      submitLabel={name.length === 0 ? "Remove from group" : "Save"}
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
          list={listId}
          value={draft}
          placeholder="e.g. Website redesign"
          onChange={(event) => setDraft(event.target.value)}
          className={FIELD}
        />
        <datalist id={listId}>
          {groups.map((group) => (
            <option key={group} value={group} />
          ))}
        </datalist>
      </Field>
    </Sheet>
  );
}
