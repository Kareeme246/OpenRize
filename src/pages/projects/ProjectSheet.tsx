import { useEffect, useState } from "react";
import { FIELD, TEXTAREA_FIELD } from "../../components/Page";
import { Picker } from "../../components/Picker";
import { Field, Sheet } from "../../components/Sheet";
import * as api from "../../lib/api";
import { describeError } from "../../lib/api";
import { addDays, localDateString, parseLocalDate } from "../../lib/dates";
import { formatDuration } from "../../lib/format";
import type { Client, HintPreview, Project } from "../../lib/types";
import { PROJECT_COLORS } from "./budget";

const PREVIEW_DAYS = 30;

export interface ProjectDraft {
  name?: string;
  aiHints?: string;
}

interface ProjectSheetProps {
  /** Editing this project; absent creates one. */
  project?: Project;
  /** Prefill for a new project, e.g. from a discovery suggestion. */
  draft?: ProjectDraft;
  clients: Client[];
  /** Colours other projects already use; a new project starts on a free one. */
  usedColors?: string[];
  onClose: () => void;
  onSaved: (project: Project) => void;
}

const KIND_LABELS: Record<string, string> = {
  path_prefix: "folder",
  url_prefix: "URL",
  domain: "domain",
  title_contains: "keyword",
};

/**
 * New or edit project: name, colour, client (pick or create inline),
 * description, AI hints with a live match preview, billing, budget, and due
 * date. Saving writes the hints' T0 rules too.
 */
export function ProjectSheet({
  project,
  draft,
  clients,
  usedColors = [],
  onClose,
  onSaved,
}: ProjectSheetProps) {
  const [name, setName] = useState(project?.name ?? draft?.name ?? "");
  const [color, setColor] = useState(
    () =>
      project?.color ??
      PROJECT_COLORS.find(
        (swatch) =>
          !usedColors.some(
            (used) => used.toLowerCase() === swatch.toLowerCase(),
          ),
      ) ??
      PROJECT_COLORS[0],
  );
  const [clientId, setClientId] = useState(project?.clientId ?? "");
  const [newClient, setNewClient] = useState<string | null>(null);
  const [description, setDescription] = useState(project?.description ?? "");
  const [hints, setHints] = useState(project?.aiHints ?? draft?.aiHints ?? "");
  const [billable, setBillable] = useState(project?.billableDefault ?? false);
  const [rate, setRate] = useState(
    project?.hourlyRate !== undefined && project?.hourlyRate !== null
      ? String(project.hourlyRate)
      : "",
  );
  const [budgetKind, setBudgetKind] = useState(project?.budgetKind ?? "none");
  const [budgetValue, setBudgetValue] = useState(
    project?.budgetValue ? String(project.budgetValue) : "",
  );
  const [budgetPeriod, setBudgetPeriod] = useState(
    project?.budgetPeriod ?? "total",
  );
  const [due, setDue] = useState(
    project?.dueDate ? localDateString(new Date(project.dueDate)) : "",
  );
  const [preview, setPreview] = useState<HintPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  // "Would have matched 14h in the last 30 days", recomputed as you type.
  useEffect(() => {
    if (!hints.trim()) {
      setPreview(null);
      return;
    }
    const timer = window.setTimeout(() => {
      const since = addDays(new Date(), -PREVIEW_DAYS).getTime();
      api
        .previewProjectHints(hints, since)
        .then(setPreview)
        .catch(() => setPreview(null));
    }, 300);
    return () => window.clearTimeout(timer);
  }, [hints]);

  const clientRate =
    clients.find((client) => client.id === clientId)?.defaultRate ?? undefined;

  const valid =
    name.trim() !== "" &&
    (budgetKind === "none" || Number(budgetValue) > 0) &&
    (newClient === null || newClient.trim() !== "");

  const submit = async (): Promise<void> => {
    setSaving(true);
    try {
      let client = clientId || null;
      if (newClient !== null) {
        client = (await api.createClient({ name: newClient.trim() })).id;
      }
      const fields = {
        name: name.trim(),
        color,
        clientId: client,
        description: description.trim() || null,
        aiHints: hints.trim() || null,
        billableDefault: billable,
        hourlyRate: rate === "" ? null : Number(rate),
        budgetKind,
        budgetValue: budgetKind === "none" ? null : Number(budgetValue),
        budgetPeriod,
        dueDate: due ? parseLocalDate(due).getTime() : null,
      };
      const saved = project
        ? await api.updateProject(project.id, fields)
        : await api.createProject({
            ...fields,
            clientId: fields.clientId ?? undefined,
            description: fields.description ?? undefined,
            aiHints: fields.aiHints ?? undefined,
            hourlyRate: fields.hourlyRate ?? undefined,
            budgetValue: fields.budgetValue ?? undefined,
            dueDate: fields.dueDate ?? undefined,
          });
      onSaved(saved);
      onClose();
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Sheet
      title={project ? `Edit ${project.name}` : "New project"}
      submitLabel={saving ? "Saving…" : project ? "Save" : "Create project"}
      canSubmit={valid && !saving}
      onSubmit={submit}
      onClose={onClose}
      error={error}
      width="lg"
    >
      <div className="grid grid-cols-[minmax(0,1fr)_auto] gap-3">
        <Field label="Name" htmlFor="project-name">
          <input
            id="project-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            // biome-ignore lint/a11y/noAutofocus: the sheet opens to type a name; otherwise the close button takes focus.
            autoFocus
            placeholder="e.g. OpenRize"
            className={FIELD}
            required
          />
        </Field>
        <Field label="Colour">
          <div className="flex h-8 items-center gap-1">
            {PROJECT_COLORS.map((swatch) => (
              <label
                key={swatch}
                className={`size-5 cursor-pointer rounded-full transition-transform has-focus-visible:ring-2 has-focus-visible:ring-accent ${
                  color === swatch
                    ? "scale-110 ring-2 ring-fg-strong ring-offset-2 ring-offset-panel"
                    : "hover:scale-110"
                }`}
                style={{ backgroundColor: swatch }}
              >
                <input
                  type="radio"
                  name="project-colour"
                  value={swatch}
                  checked={color === swatch}
                  onChange={() => setColor(swatch)}
                  aria-label={`Colour ${swatch}`}
                  className="sr-only"
                />
              </label>
            ))}
          </div>
        </Field>
      </div>

      <Field label="Client" htmlFor="project-client">
        {newClient === null ? (
          <Picker
            id="project-client"
            ariaLabel="Client"
            value={clientId}
            onChange={(val) => {
              if (val === "__new") setNewClient("");
              else setClientId(val);
            }}
            options={[
              { value: "", label: "No client" },
              ...clients.map((client) => ({
                value: client.id,
                label: client.name,
              })),
              { value: "__new", label: "+ New client…" },
            ]}
            variant="field"
          />
        ) : (
          <div className="flex gap-2">
            <input
              id="project-client"
              // biome-ignore lint/a11y/noAutofocus: chosen from the client picker to type a name.
              autoFocus
              value={newClient}
              onChange={(event) => setNewClient(event.target.value)}
              placeholder="New client name"
              className={FIELD}
            />
            <button
              type="button"
              onClick={() => setNewClient(null)}
              className="shrink-0 text-[12px] text-fg-soft hover:text-fg"
            >
              Cancel
            </button>
          </div>
        )}
      </Field>

      <Field label="Description" htmlFor="project-description">
        <input
          id="project-description"
          value={description}
          onChange={(event) => setDescription(event.target.value)}
          className={FIELD}
        />
      </Field>

      <Field
        label="AI hints"
        htmlFor="project-hints"
        hint="Keywords, folders, repos, and domains, separated by commas. Each becomes a rule that assigns matching time to this project."
      >
        <textarea
          id="project-hints"
          value={hints}
          onChange={(event) => setHints(event.target.value)}
          placeholder="~/Code/OpenRize, github.com/you/OpenRize, openrize.app"
          rows={2}
          className={TEXTAREA_FIELD}
        />
      </Field>
      {preview && preview.hints.length > 0 && (
        <div className="rounded-lg border border-line bg-inset-soft p-2.5">
          <p className="text-[11.5px] text-fg">
            Would have matched{" "}
            <b className="text-fg-strong">{formatDuration(preview.totalMs)}</b>{" "}
            in the last {PREVIEW_DAYS} days
          </p>
          <ul className="mt-1.5 space-y-0.5">
            {preview.hints.map((hint) => (
              <li
                key={hint.hint}
                className="flex items-center gap-2 text-[11px] text-fg-soft"
              >
                <span className="w-16 shrink-0 text-fg-faint">
                  {hint.matchKind ? KIND_LABELS[hint.matchKind] : "too short"}
                </span>
                <span className="min-w-0 flex-1 truncate font-mono text-fg-muted">
                  {hint.hint}
                </span>
                <span
                  className={`shrink-0 font-mono tabular-nums ${
                    hint.matchedMs > 0 ? "text-fg-muted" : "text-fg-faint"
                  }`}
                >
                  {hint.matchKind ? formatDuration(hint.matchedMs) : "–"}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        <Field
          label="Hourly rate"
          htmlFor="project-rate"
          hint="Falls back to the client's rate"
        >
          <input
            id="project-rate"
            type="number"
            min="0"
            step="any"
            value={rate}
            onChange={(event) => setRate(event.target.value)}
            placeholder={clientRate === undefined ? "None" : String(clientRate)}
            className={FIELD}
          />
        </Field>
        <Field label="Billing">
          <label className="flex h-8 items-center gap-2 text-fg-muted">
            <input
              type="checkbox"
              checked={billable}
              onChange={(event) => setBillable(event.target.checked)}
              className="accent-(--accent)"
            />
            New time is billable
          </label>
        </Field>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <Field label="Budget" htmlFor="project-budget-kind">
          <Picker
            id="project-budget-kind"
            ariaLabel="Budget"
            value={budgetKind}
            onChange={(val) => setBudgetKind(val)}
            options={[
              { value: "none", label: "None" },
              { value: "hours", label: "Hours" },
              { value: "amount", label: "Amount" },
            ]}
            variant="field"
          />
        </Field>
        <Field
          label={budgetKind === "amount" ? "Amount" : "Hours"}
          htmlFor="project-budget-value"
        >
          <input
            id="project-budget-value"
            type="number"
            min="0"
            step="any"
            disabled={budgetKind === "none"}
            value={budgetValue}
            onChange={(event) => setBudgetValue(event.target.value)}
            className={`${FIELD} disabled:opacity-40`}
          />
        </Field>
        <Field label="Per" htmlFor="project-budget-period">
          <Picker
            id="project-budget-period"
            ariaLabel="Per"
            value={budgetPeriod}
            disabled={budgetKind === "none"}
            onChange={(val) => setBudgetPeriod(val)}
            options={[
              { value: "total", label: "Total" },
              { value: "monthly", label: "Month" },
            ]}
            variant="field"
          />
        </Field>
      </div>

      <Field label="Due date" htmlFor="project-due">
        <input
          id="project-due"
          type="date"
          value={due}
          onChange={(event) => setDue(event.target.value)}
          className={`${FIELD} max-w-48`}
        />
      </Field>
    </Sheet>
  );
}
