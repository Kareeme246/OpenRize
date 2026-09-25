import { type ReactNode, useEffect, useMemo, useState } from "react";
import {
  BAND_FILL,
  BAND_LABEL,
  BAND_TONE,
  band,
  HIGH,
  METER_SEGMENTS,
  percent,
} from "../lib/confidence";
import type {
  Category,
  EntryDetail,
  FieldSuggestion,
  Project,
  RuleSuggestion,
  SuggestionField,
} from "../lib/types";
import { Picker } from "./Picker";

/** One pickable value for a field, ranked: suggestion, alternatives, rest. */
interface PickOption {
  /** `null` is "No project". */
  valueId: string | null;
  name: string;
  color: string;
  confidence?: number;
}

interface FieldModel {
  field: SuggestionField;
  /** The latest suggestion, unless the user rejected it. */
  suggestion?: FieldSuggestion;
  rejected: boolean;
  value: string | null;
  options: PickOption[];
}

/** Number keys 1-9 pick from the active field's first nine options. */
const KEYED_OPTIONS = 9;
/** Options shown as full rows (with their confidence) before the chips. */
const ROW_OPTIONS = 4;
const NO_PROJECT_COLOR = "var(--fg-faint)";

const EVENT_LABELS: Record<string, string> = {
  created: "Created",
  suggested: "Suggested",
  accepted: "Accepted",
  rejected: "Rejected",
  edited: "Edited",
  split: "Split",
  merged: "Merged",
  recategorized: "Recategorized",
  auto_approved: "Auto-approved",
};

const ACTOR_LABELS: Record<string, string> = {
  user: "you",
  ai: "AI",
  rule: "rule",
  auto: "AI",
  system: "OpenRize",
};

export interface EntryReviewPanelProps {
  detail: EntryDetail;
  categories: Category[];
  projects: Project[];
  /** Settings → AI: suggest projects too, not just categories. */
  suggestProjects: boolean;
  /** "3 of 7" while stepping through pending entries in review mode. */
  reviewPosition?: { index: number; total: number };
  onClose: () => void;
  onAccept: () => void;
  onReject: () => void;
  onSplit: () => void;
  onDelete: () => void;
  onRetry: () => void;
  onSetField: (field: SuggestionField, valueId: string | null) => void;
  onToggleBillable: () => void;
  onSaveDescription: (description: string) => void;
  onResolveRule: (suggestion: RuleSuggestion, accept: boolean) => void;
  formatTime: (epochMs: number) => string;
  formatDuration: (ms: number) => string;
}

function buildOptions(
  field: SuggestionField,
  suggestion: FieldSuggestion | undefined,
  categories: Category[],
  projects: Project[],
): PickOption[] {
  const choices: PickOption[] =
    field === "category"
      ? categories
          .filter((category) => !category.archived)
          .map((category) => ({
            valueId: category.id,
            name: category.name,
            color: category.color,
          }))
      : [
          { valueId: null, name: "No project", color: NO_PROJECT_COLOR },
          ...projects
            .filter((project) => project.status === "active")
            .map((project) => ({
              valueId: project.id,
              name: project.name,
              color: project.color,
            })),
        ];

  const ranked: PickOption[] = [];
  const take = (valueId: string | null, confidence: number): void => {
    const choice = choices.find((option) => option.valueId === valueId);
    if (choice && !ranked.some((option) => option.valueId === valueId)) {
      ranked.push({ ...choice, confidence });
    }
  };
  if (suggestion) {
    take(suggestion.valueId ?? null, suggestion.confidence);
    for (const alternative of suggestion.alternatives) {
      take(alternative.valueId ?? null, alternative.confidence);
    }
  }
  for (const choice of choices) {
    if (!ranked.some((option) => option.valueId === choice.valueId)) {
      ranked.push(choice);
    }
  }
  return ranked;
}

/** A field is settled when it has a value nobody needs to double-check. */
function settled(model: FieldModel): boolean {
  if (model.value === null && model.field === "category") return false;
  const suggestion = model.suggestion;
  if (!suggestion) return model.value !== null;
  if (suggestion.outcome !== undefined) return true;
  const matches = (suggestion.valueId ?? null) === model.value;
  return (
    matches && (suggestion.engine === "rules" || suggestion.confidence >= HIGH)
  );
}

function isTyping(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  return (
    element?.tagName === "INPUT" ||
    element?.tagName === "TEXTAREA" ||
    element?.tagName === "SELECT" ||
    element?.isContentEditable === true
  );
}

export function EntryReviewPanel({
  detail,
  categories,
  projects,
  suggestProjects,
  reviewPosition,
  onClose,
  onAccept,
  onReject,
  onSplit,
  onDelete,
  onRetry,
  onSetField,
  onToggleBillable,
  onSaveDescription,
  onResolveRule,
  formatTime,
  formatDuration,
}: EntryReviewPanelProps) {
  const { entry, job } = detail;
  const [tab, setTab] = useState<"apps" | "titles" | "log" | "history">("apps");
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(entry.description);

  const models = useMemo<FieldModel[]>(() => {
    const fields: SuggestionField[] = suggestProjects
      ? ["category", "project"]
      : ["category"];
    return fields.map((field) => {
      const latest = detail.suggestions.find((s) => s.field === field);
      const rejected = latest?.outcome === "rejected";
      const suggestion = rejected ? undefined : latest;
      return {
        field,
        suggestion,
        rejected,
        value:
          (field === "category" ? entry.categoryId : entry.projectId) ?? null,
        options: buildOptions(field, suggestion, categories, projects),
      };
    });
  }, [detail, entry, categories, projects, suggestProjects]);

  // The panel pre-fills the confident field and asks about the other: start
  // on the first field that still needs a decision.
  const firstOpenField =
    models.find((model) => !settled(model))?.field ?? "category";
  const [activeField, setActiveField] =
    useState<SuggestionField>(firstOpenField);
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-aim only when a different entry opens, not on every edit.
  useEffect(() => {
    setActiveField(firstOpenField);
    setEditing(false);
    setDraft(entry.description);
  }, [entry.id]);

  const active = models.find((model) => model.field === activeField);
  const processing =
    entry.status === "processing" ||
    job?.state === "queued" ||
    job?.state === "running";
  const approved = entry.status === "approved";

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (isTyping(event.target)) return;
      const key = event.key.toLowerCase();
      if (/^[1-9]$/.test(key) && active && !approved) {
        const option = active.options[Number(key) - 1];
        if (option) {
          event.preventDefault();
          onSetField(active.field, option.valueId);
        }
      } else if (key === "c") {
        event.preventDefault();
        setActiveField("category");
      } else if (key === "p" && suggestProjects) {
        event.preventDefault();
        setActiveField("project");
      } else if (key === "e") {
        event.preventDefault();
        setEditing(true);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [active, approved, onSetField, suggestProjects]);

  const nameOf = (field: SuggestionField, valueId?: string): string => {
    if (field === "category") {
      return categories.find((c) => c.id === valueId)?.name ?? "Unknown";
    }
    if (!valueId) return "No project";
    return projects.find((p) => p.id === valueId)?.name ?? "Unknown";
  };

  const saveDescription = (): void => {
    onSaveDescription(draft.trim());
    setEditing(false);
  };

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <span className="text-[13px] font-semibold text-fg-strong">
          Review entry
          {reviewPosition && (
            <span className="ml-2 font-normal text-fg-faint text-[11.5px]">
              {reviewPosition.index + 1} of {reviewPosition.total}
            </span>
          )}
        </span>
        <button
          type="button"
          onClick={onClose}
          className="rounded p-1 text-fg-faint transition-colors hover:bg-surface hover:text-fg"
          title="Close (Esc)"
        >
          ✕
        </button>
      </div>

      <div className="flex-1 space-y-3 overflow-y-auto p-4">
        <div className="flex items-center justify-between text-[12px] text-fg-soft">
          <span>
            {formatTime(entry.startedAt)}–{formatTime(entry.endedAt)} ·{" "}
            {formatDuration(entry.endedAt - entry.startedAt)}
          </span>
          <button
            type="button"
            onClick={onToggleBillable}
            className={`rounded px-2 py-0.5 text-[11.5px] font-medium transition-colors ${
              entry.billable
                ? "bg-accent/20 font-semibold text-accent"
                : "bg-surface text-fg-faint hover:text-fg"
            }`}
          >
            $ Billable
          </button>
        </div>

        <div className="rounded-lg border border-line bg-surface p-2.5">
          {editing ? (
            <div className="space-y-2">
              <textarea
                // biome-ignore lint/a11y/noAutofocus: E opens the editor to type into.
                autoFocus
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault();
                    saveDescription();
                  } else if (event.key === "Escape") {
                    event.stopPropagation();
                    setEditing(false);
                  }
                }}
                className="w-full rounded border border-line bg-canvas px-2 py-1.5 text-[12.5px] text-fg outline-hidden focus:border-accent"
                rows={2}
              />
              <div className="flex justify-end gap-1.5">
                <button
                  type="button"
                  onClick={() => setEditing(false)}
                  className="rounded px-2 py-0.5 text-[11px] text-fg-soft hover:text-fg"
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={saveDescription}
                  className="rounded bg-accent px-2.5 py-0.5 font-semibold text-[11px] text-accent-fg"
                >
                  Save
                </button>
              </div>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setEditing(true)}
              className="w-full cursor-pointer text-left font-medium text-[12.5px] text-fg-strong transition-colors hover:text-accent"
              title="Edit description (E)"
            >
              {entry.description}
              {entry.descriptionOrigin === "ai" && (
                <span className="ml-1.5 rounded-sm border border-line px-1 align-middle font-normal text-[9.5px] text-fg-faint">
                  AI
                </span>
              )}
            </button>
          )}
        </div>

        {job?.state === "failed" && !approved && (
          <div
            role="alert"
            className="flex items-center gap-2 rounded-lg border border-danger/30 bg-danger-soft px-3 py-2 text-[11.5px]"
          >
            <span className="min-w-0 flex-1 text-danger" title={job.lastError}>
              Couldn't categorize this entry
            </span>
            <button
              type="button"
              onClick={onRetry}
              className="shrink-0 rounded-md border border-danger/40 px-2 py-0.5 font-semibold text-danger hover:bg-danger/10"
            >
              Retry
            </button>
          </div>
        )}

        {models.map((model) => (
          <FieldSection
            key={model.field}
            model={model}
            active={model.field === activeField}
            processing={processing}
            locked={approved}
            onActivate={() => setActiveField(model.field)}
            onPick={(valueId) => onSetField(model.field, valueId)}
            nameOf={(valueId) => nameOf(model.field, valueId)}
          />
        ))}

        {detail.ruleSuggestion && !approved && (
          <RulePrompt
            suggestion={detail.ruleSuggestion}
            valueName={nameOf(
              detail.ruleSuggestion.field,
              detail.ruleSuggestion.valueId,
            )}
            onResolve={(accept) =>
              detail.ruleSuggestion &&
              onResolveRule(detail.ruleSuggestion, accept)
            }
          />
        )}

        <div className="flex items-center gap-1.5 border-line border-t pt-3">
          <button
            type="button"
            onClick={onAccept}
            disabled={!entry.categoryId || approved}
            title={
              approved
                ? "Already approved"
                : entry.categoryId
                  ? "Accept (⌘↵)"
                  : "Pick a category first (1–9)"
            }
            className="whitespace-nowrap flex-1 rounded-md bg-accent px-3 py-1.5 font-semibold text-[12px] text-accent-fg transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {approved ? "Approved ✓" : "Accept"}{" "}
            {!approved && <span className="text-[10px] opacity-70">⌘↵</span>}
          </button>
          <button
            type="button"
            onClick={onReject}
            disabled={approved}
            className="whitespace-nowrap rounded-md border border-line bg-surface px-2 py-1.5 font-medium text-[12px] text-fg-soft transition-colors hover:bg-surface-strong hover:text-fg disabled:cursor-not-allowed disabled:opacity-40"
          >
            Reject <span className="text-[10px] opacity-70">⌘⌫</span>
          </button>
          <button
            type="button"
            onClick={onSplit}
            className="whitespace-nowrap rounded-md border border-line bg-surface px-2 py-1.5 text-[12px] text-fg-soft transition-colors hover:bg-surface-strong hover:text-fg"
            title="Split at the main app switch (S)"
          >
            Split
          </button>
          <button
            type="button"
            onClick={onDelete}
            className="whitespace-nowrap rounded-md border border-line bg-surface px-2 py-1.5 text-[12px] text-danger transition-colors hover:bg-danger/10"
            title="Delete entry"
          >
            ✕
          </button>
        </div>
        <p className="text-[10.5px] text-fg-faint">
          <Kbd>1–9</Kbd> pick · <Kbd>C</Kbd>
          {suggestProjects && (
            <>
              /<Kbd>P</Kbd>
            </>
          )}{" "}
          field · <Kbd>J</Kbd>/<Kbd>K</Kbd> next · <Kbd>E</Kbd> edit
        </p>

        <div className="pt-1">
          <div className="flex border-line border-b font-medium text-[11px]">
            {(["apps", "titles", "log", "history"] as const).map((name) => (
              <button
                key={name}
                type="button"
                onClick={() => setTab(name)}
                className={`flex-1 border-b-2 pb-1.5 text-center capitalize transition-colors ${
                  tab === name
                    ? "border-accent font-semibold text-accent"
                    : "border-transparent text-fg-soft hover:text-fg"
                }`}
              >
                {name}
              </button>
            ))}
          </div>

          <div className="pt-3 text-[11.5px]">
            {tab === "apps" && (
              <div className="space-y-2">
                {detail.apps.map((app) => (
                  <div key={app.app} className="space-y-1">
                    <div className="flex justify-between text-fg-soft">
                      <span className="truncate pr-2">{app.app}</span>
                      <span className="shrink-0 font-mono text-fg-faint">
                        {formatDuration(app.durationMs)} (
                        {Math.round(app.percentage)}%)
                      </span>
                    </div>
                    <div className="h-1.5 w-full overflow-hidden rounded-full bg-surface">
                      <div
                        className="h-full rounded-full bg-accent"
                        style={{ width: `${app.percentage}%` }}
                      />
                    </div>
                  </div>
                ))}
                {detail.apps.length === 0 && <Empty>No app activity</Empty>}
              </div>
            )}

            {tab === "titles" && (
              <div className="max-h-48 space-y-2 overflow-y-auto">
                {detail.titles.map((title) => (
                  <div
                    key={`${title.app}-${title.startedAt}-${title.title.slice(0, 20)}`}
                    className="border-line-soft border-b pb-1"
                  >
                    <div className="truncate text-fg-strong">{title.title}</div>
                    <div className="text-[10px] text-fg-faint">
                      {title.app} · {formatDuration(title.durationMs)}
                    </div>
                  </div>
                ))}
                {detail.titles.length === 0 && (
                  <Empty>No titles captured</Empty>
                )}
              </div>
            )}

            {tab === "log" && (
              <div className="max-h-48 space-y-1 overflow-y-auto font-mono text-[10.5px]">
                {detail.segments.map((segment) => (
                  <div
                    key={segment.id}
                    className="flex justify-between text-fg-soft"
                  >
                    <span className="truncate pr-2">
                      {formatTime(segment.startedAt)} {segment.app}
                    </span>
                    <span className="shrink-0 text-fg-faint">
                      {segment.endedAt
                        ? formatDuration(segment.endedAt - segment.startedAt)
                        : "active"}
                    </span>
                  </div>
                ))}
              </div>
            )}

            {tab === "history" && (
              <div className="max-h-48 space-y-1 overflow-y-auto text-[11px]">
                {detail.events.map((event) => (
                  <div
                    key={event.id}
                    className="flex items-center justify-between text-fg-soft"
                  >
                    <span>
                      {EVENT_LABELS[event.kind] ?? event.kind} by{" "}
                      {ACTOR_LABELS[event.actor] ?? event.actor}
                    </span>
                    <span className="font-mono text-[10px] text-fg-faint">
                      {formatTime(event.at)}
                    </span>
                  </div>
                ))}
                {detail.events.length === 0 && <Empty>No history yet</Empty>}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function FieldSection({
  model,
  active,
  processing,
  locked,
  onActivate,
  onPick,
  nameOf,
}: {
  model: FieldModel;
  active: boolean;
  processing: boolean;
  locked: boolean;
  onActivate: () => void;
  onPick: (valueId: string | null) => void;
  nameOf: (valueId?: string) => string;
}) {
  const { field, suggestion, value, options } = model;
  const current = options.find((option) => option.valueId === value);
  const suggested = suggestion ? (suggestion.valueId ?? null) : undefined;
  const changed = suggested !== undefined && suggested !== value;
  const keyed = options.slice(0, KEYED_OPTIONS);
  const rows = keyed.filter(
    (option, index) => index < ROW_OPTIONS && option.confidence !== undefined,
  );
  const chips = keyed.slice(rows.length);
  const overflow = options.slice(KEYED_OPTIONS);
  const label = field === "category" ? "Category" : "Project";
  const shortcut = field === "category" ? "C" : "P";

  return (
    <section
      className={`rounded-lg border p-2.5 transition-colors ${
        active && !locked ? "border-accent/50 bg-accent/5" : "border-line"
      }`}
    >
      <button
        type="button"
        onClick={onActivate}
        className="flex w-full items-center gap-2 text-left"
        title={`Choose ${label.toLowerCase()} (${shortcut})`}
      >
        <span className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
          {label}
        </span>
        <Kbd>{shortcut}</Kbd>
        <span className="ml-auto flex items-center gap-2">
          {processing && !suggestion ? (
            <span className="animate-pulse text-[11px] text-fg-soft">
              Categorizing…
            </span>
          ) : suggestion ? (
            <ConfidenceSummary suggestion={suggestion} />
          ) : model.rejected ? (
            <span className="text-[11px] text-fg-faint">Rejected</span>
          ) : null}
        </span>
      </button>

      <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        {current ? (
          <Chip option={current} strong />
        ) : (
          <span className="text-[12px] text-fg-faint italic">
            {field === "category" ? "No category yet" : "No project"}
          </span>
        )}
        {changed && (
          <span className="text-[10.5px] text-fg-faint">
            AI suggested {nameOf(suggested ?? undefined)}
          </span>
        )}
        {suggestion && !changed && value !== null && (
          <span className="text-[10.5px] text-fg-faint">
            {suggestion.outcome === "auto" ? "auto-approved" : "suggested"}
          </span>
        )}
      </div>

      {suggestion?.rationale && (
        <p className="mt-2 rounded-md bg-inset-soft px-2 py-1.5 text-[11px] text-fg-muted leading-snug">
          <span className="font-semibold text-fg-soft">Why: </span>
          {suggestion.rationale}
        </p>
      )}

      {active && !locked && (
        <div className="mt-2 space-y-1">
          {rows.map((option, index) => (
            <OptionRow
              key={option.valueId ?? "none"}
              option={option}
              shortcut={index + 1}
              selected={option.valueId === value}
              onPick={onPick}
            />
          ))}
          {chips.length > 0 && (
            <div className="flex flex-wrap gap-1 pt-0.5">
              {chips.map((option, index) => (
                <button
                  key={option.valueId ?? "none"}
                  type="button"
                  onClick={() => onPick(option.valueId)}
                  className={`inline-flex max-w-full items-center gap-1.5 rounded-md border px-1.5 py-0.5 text-[11px] transition-colors ${
                    option.valueId === value
                      ? "border-accent/60 bg-accent-soft text-fg-strong"
                      : "border-line text-fg-muted hover:bg-surface-strong"
                  }`}
                >
                  <Dot color={option.color} />
                  <span className="truncate">{option.name}</span>
                  <span className="font-mono text-[9.5px] text-fg-faint">
                    {rows.length + index + 1}
                  </span>
                </button>
              ))}
            </div>
          )}
          {overflow.length > 0 && (
            <Picker
              ariaLabel={`More ${label.toLowerCase()} options`}
              value=""
              placeholder={`More ${label.toLowerCase()}…`}
              onChange={(val) => onPick(val === "" ? null : val)}
              options={[
                {
                  value: "",
                  label: `More ${label.toLowerCase()}…`,
                  disabled: true,
                },
                ...overflow.map((option) => ({
                  value: option.valueId ?? "",
                  label: option.name,
                  color: option.color,
                })),
              ]}
              variant="compact"
              className="mt-1 w-full"
            />
          )}
        </div>
      )}
    </section>
  );
}

function ConfidenceSummary({ suggestion }: { suggestion: FieldSuggestion }) {
  if (suggestion.engine === "rules") {
    return <span className="font-medium text-[11px] text-accent">Rule ✓</span>;
  }
  const level = band(suggestion.confidence);
  return (
    <>
      <Meter confidence={suggestion.confidence} />
      <span
        className={`font-medium text-[11px] tabular-nums ${BAND_TONE[level]}`}
      >
        {percent(suggestion.confidence)} · {BAND_LABEL[level]}
      </span>
    </>
  );
}

function Meter({ confidence }: { confidence: number }) {
  const level = band(confidence);
  const filled = Math.round(confidence * METER_SEGMENTS);
  return (
    // Decorative: the percentage and band label beside it carry the value.
    <span className="flex gap-0.5" aria-hidden="true">
      {Array.from({ length: METER_SEGMENTS }, (_, index) => index).map(
        (index) => (
          <i
            key={`meter-${index}`}
            className={`block h-1.5 w-3 rounded-xs ${
              index < filled ? BAND_FILL[level] : "bg-surface-strong"
            }`}
          />
        ),
      )}
    </span>
  );
}

function OptionRow({
  option,
  shortcut,
  selected,
  onPick,
}: {
  option: PickOption;
  shortcut: number;
  selected: boolean;
  onPick: (valueId: string | null) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onPick(option.valueId)}
      className={`flex w-full items-center gap-2 rounded-md border px-2 py-1 text-left text-[11.5px] transition-colors ${
        selected
          ? "border-accent/60 bg-accent-soft text-fg-strong"
          : "border-line-soft text-fg-muted hover:bg-surface-strong"
      }`}
    >
      <Dot color={option.color} />
      <span className="min-w-0 flex-1 truncate">{option.name}</span>
      {selected && <span className="text-accent text-[10.5px]">✓</span>}
      {option.confidence !== undefined && (
        <span className="font-mono text-[10.5px] text-fg-soft tabular-nums">
          {percent(option.confidence)}
        </span>
      )}
      <Kbd>{shortcut}</Kbd>
    </button>
  );
}

function RulePrompt({
  suggestion,
  valueName,
  onResolve,
}: {
  suggestion: RuleSuggestion;
  valueName: string;
  onResolve: (accept: boolean) => void;
}) {
  const verb = suggestion.field === "category" ? "categorize" : "assign";
  const joiner = suggestion.field === "category" ? "as" : "to";
  return (
    <div className="rounded-lg border border-accent/30 bg-accent-soft p-2.5 text-[11.5px]">
      <p className="text-fg">
        Always {verb} <b className="text-fg-strong">{suggestion.label}</b>{" "}
        {joiner} <b className="text-fg-strong">{valueName}</b>?
      </p>
      <p className="mt-0.5 text-[10.5px] text-fg-soft">
        You've corrected this {suggestion.corrections} times. A rule applies it
        automatically from now on.
      </p>
      <div className="mt-2 flex gap-1.5">
        <button
          type="button"
          onClick={() => onResolve(true)}
          className="rounded-md bg-accent px-2.5 py-1 font-semibold text-[11px] text-accent-fg hover:opacity-90"
        >
          Create rule
        </button>
        <button
          type="button"
          onClick={() => onResolve(false)}
          className="rounded-md border border-line px-2.5 py-1 text-[11px] text-fg-soft hover:bg-surface-strong hover:text-fg"
        >
          Dismiss
        </button>
      </div>
    </div>
  );
}

function Chip({ option, strong }: { option: PickOption; strong?: boolean }) {
  return (
    <span
      className={`inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-md border px-2 py-0.5 text-[12px] ${
        strong ? "font-medium text-fg-strong" : "text-fg-muted"
      }`}
      style={{
        borderColor: `color-mix(in srgb, ${option.color} 50%, transparent)`,
      }}
    >
      <Dot color={option.color} />
      <span className="truncate">{option.name}</span>
    </span>
  );
}

function Dot({ color }: { color: string }) {
  return (
    <span
      className="size-2 shrink-0 rounded-xs"
      style={{ backgroundColor: color }}
    />
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded border border-line px-1 font-mono text-[9.5px] text-fg-faint">
      {children}
    </kbd>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return <div className="py-2 text-center text-fg-faint">{children}</div>;
}
