import { useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { localDateString, parseLocalDate } from "../lib/dates";
import type { Category, Project } from "../lib/types";
import { FIELD } from "./Page";
import { Picker } from "./Picker";
import { Field, Sheet } from "./Sheet";

interface AddTimeSheetProps {
  date: Date;
  categories: Category[];
  projects: Project[];
  onClose: () => void;
  onCreated: () => void;
}

/** `HH:MM` on a local date, as epoch ms. */
function at(day: string, time: string): number {
  const [hours, minutes] = time.split(":").map(Number);
  const date = parseLocalDate(day);
  date.setHours(hours, minutes, 0, 0);
  return date.getTime();
}

/**
 * The manual entry sheet: start, end, category, and description (plus an
 * optional project and billable). A hand-made entry is approved as made.
 */
export function AddTimeSheet({
  date,
  categories,
  projects,
  onClose,
  onCreated,
}: AddTimeSheetProps) {
  const [day, setDay] = useState(localDateString(date));
  const [start, setStart] = useState("09:00");
  const [end, setEnd] = useState("10:00");
  const [description, setDescription] = useState("");
  const [categoryId, setCategoryId] = useState("");
  const [projectId, setProjectId] = useState("");
  const [billable, setBillable] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const startMs = at(day, start);
  const endMs = at(day, end);
  const valid =
    description.trim() !== "" && categoryId !== "" && endMs > startMs;

  const pickProject = (id: string): void => {
    setProjectId(id);
    const project = projects.find((candidate) => candidate.id === id);
    if (project) setBillable(project.billableDefault);
  };

  const submit = async (): Promise<void> => {
    setSaving(true);
    try {
      await api.createTimeEntry({
        startedAt: startMs,
        endedAt: endMs,
        description: description.trim(),
        categoryId,
        projectId: projectId || undefined,
        billable,
      });
      onCreated();
      onClose();
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Sheet
      title="Add time"
      submitLabel={saving ? "Adding…" : "Add entry"}
      canSubmit={valid && !saving}
      onSubmit={submit}
      onClose={onClose}
      error={
        error ??
        (endMs <= startMs ? "The end time must be after the start." : null)
      }
      width="sm"
    >
      <div className="grid grid-cols-3 gap-2">
        <Field label="Date" htmlFor="add-time-day">
          <input
            id="add-time-day"
            type="date"
            value={day}
            onChange={(event) => setDay(event.target.value)}
            className={FIELD}
            required
          />
        </Field>
        <Field label="Start" htmlFor="add-time-start">
          <input
            id="add-time-start"
            type="time"
            value={start}
            onChange={(event) => setStart(event.target.value)}
            className={FIELD}
            required
          />
        </Field>
        <Field label="End" htmlFor="add-time-end">
          <input
            id="add-time-end"
            type="time"
            value={end}
            onChange={(event) => setEnd(event.target.value)}
            className={FIELD}
            required
          />
        </Field>
      </div>
      <Field label="Description" htmlFor="add-time-description">
        <input
          id="add-time-description"
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          placeholder="What did you work on?"
          className={FIELD}
          required
        />
      </Field>
      <div className="grid grid-cols-2 gap-2">
        <Field label="Category" htmlFor="add-time-category">
          <Picker
            id="add-time-category"
            ariaLabel="Category"
            value={categoryId}
            onChange={setCategoryId}
            placeholder="Choose…"
            options={[
              { value: "", label: "Choose…", disabled: true },
              ...categories
                .filter((category) => !category.archived)
                .map((category) => ({
                  value: category.id,
                  label: category.name,
                  color: category.color,
                })),
            ]}
            variant="field"
          />
        </Field>
        <Field label="Project" htmlFor="add-time-project">
          <Picker
            id="add-time-project"
            ariaLabel="Project"
            value={projectId}
            onChange={pickProject}
            options={[
              { value: "", label: "No project" },
              ...projects
                .filter((project) => project.status === "active")
                .map((project) => ({
                  value: project.id,
                  label: project.name,
                  color: project.color,
                })),
            ]}
            variant="field"
          />
        </Field>
      </div>
      <label className="flex items-center gap-2 text-fg-muted">
        <input
          type="checkbox"
          checked={billable}
          onChange={(event) => setBillable(event.target.checked)}
          className="accent-(--accent)"
        />
        Billable
      </label>
    </Sheet>
  );
}
