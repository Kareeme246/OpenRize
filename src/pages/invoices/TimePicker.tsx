import { useEffect, useMemo, useState } from "react";
import { FilterSelect } from "../../components/Page";
import { Sheet } from "../../components/Sheet";
import * as api from "../../lib/api";
import { addDays, startOfDay, startOfMonth } from "../../lib/dates";
import { formatQuantity, formatUsd } from "../../lib/invoices";
import type { BillableEntry } from "../../lib/types";

type Span = "all" | "month" | "last-month";

const SPANS: { value: Span; label: string }[] = [
  { value: "all", label: "All time" },
  { value: "month", label: "This month" },
  { value: "last-month", label: "Last month" },
];

function spanBounds(span: Span): { start: number; end: number } {
  const now = new Date();
  const tomorrow = addDays(startOfDay(now), 1).getTime();
  const thisMonth = startOfMonth(now);
  if (span === "month") return { start: thisMonth.getTime(), end: tomorrow };
  if (span === "last-month") {
    const previous = new Date(
      thisMonth.getFullYear(),
      thisMonth.getMonth() - 1,
    );
    return { start: previous.getTime(), end: thisMonth.getTime() };
  }
  return { start: 0, end: tomorrow };
}

interface TimePickerProps {
  clientId: string;
  invoiceId?: string;
  /** Entries already on the draft, hidden from the list. */
  excluded: ReadonlySet<string>;
  /** Start narrowed to one project, e.g. when invoicing from a project. */
  projectId?: string;
  onAdd: (entries: BillableEntry[]) => void;
  onClose: () => void;
}

/**
 * Auto-populate from tracked time: every approved, billable, uninvoiced entry
 * for the client, narrowed by period and project, all preselected. Each chosen
 * entry becomes its own invoice line at the rate its project (or client) bills.
 */
export function TimePicker({
  clientId,
  invoiceId,
  excluded,
  projectId,
  onAdd,
  onClose,
}: TimePickerProps) {
  const [span, setSpan] = useState<Span>("all");
  const [project, setProject] = useState(projectId ?? "");
  const [entries, setEntries] = useState<BillableEntry[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setEntries(null);
    const { start, end } = spanBounds(span);
    api
      .listBillableEntries(clientId, start, end, invoiceId)
      .then((found) => {
        if (cancelled) return;
        const available = found.filter((e) => !excluded.has(e.entryId));
        setEntries(available);
        setSelected(new Set(available.map((e) => e.entryId)));
        setError(null);
      })
      .catch((cause) => {
        if (!cancelled) setError(api.describeError(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [clientId, invoiceId, span, excluded]);

  const projects = useMemo(() => {
    const names = new Map<string, string>();
    for (const entry of entries ?? []) {
      names.set(entry.projectId, entry.projectName);
    }
    return [...names].map(([value, label]) => ({ value, label }));
  }, [entries]);

  const visible = useMemo(
    () => (entries ?? []).filter((e) => !project || e.projectId === project),
    [entries, project],
  );
  const groups = useMemo(() => {
    const byProject = new Map<string, BillableEntry[]>();
    for (const entry of visible) {
      const list = byProject.get(entry.projectId) ?? [];
      list.push(entry);
      byProject.set(entry.projectId, list);
    }
    return [...byProject.values()];
  }, [visible]);

  const chosen = visible.filter((e) => selected.has(e.entryId));
  const hundredths = chosen.reduce((sum, e) => sum + e.quantityHundredths, 0);
  const amount = chosen.reduce((sum, e) => sum + (e.amountCents ?? 0), 0);
  const unrated = chosen.filter((e) => e.rateCents == null).length;

  const toggle = (ids: string[], on: boolean): void =>
    setSelected((previous) => {
      const next = new Set(previous);
      for (const id of ids) {
        if (on) next.add(id);
        else next.delete(id);
      }
      return next;
    });

  return (
    <Sheet
      title="Add tracked time"
      submitLabel={
        chosen.length === 0
          ? "Add time"
          : `Add ${chosen.length} ${chosen.length === 1 ? "entry" : "entries"}`
      }
      onSubmit={() => {
        onAdd(chosen);
        onClose();
      }}
      onClose={onClose}
      canSubmit={chosen.length > 0}
      error={error}
      width="lg"
    >
      <div className="flex flex-wrap items-center gap-2">
        <FilterSelect
          label="Period"
          value={span}
          options={SPANS}
          onChange={(value) => setSpan(value as Span)}
        />
        <FilterSelect
          label="All projects"
          value={project}
          options={projects}
          onChange={setProject}
        />
        <span className="ml-auto flex gap-3 text-[11.5px]">
          <button
            type="button"
            className="text-fg-soft underline hover:text-fg"
            onClick={() =>
              toggle(
                visible.map((e) => e.entryId),
                true,
              )
            }
          >
            Select all
          </button>
          <button
            type="button"
            className="text-fg-soft underline hover:text-fg"
            onClick={() =>
              toggle(
                visible.map((e) => e.entryId),
                false,
              )
            }
          >
            None
          </button>
        </span>
      </div>

      {entries === null ? (
        <p className="py-8 text-center text-fg-faint">
          Finding billable time...
        </p>
      ) : visible.length === 0 ? (
        <p className="py-8 text-center text-fg-faint">
          No approved billable time is waiting to be invoiced for this client
          {span === "all" ? "" : " in this period"}. Approve entries on the
          Timesheet first.
        </p>
      ) : (
        <div className="space-y-3">
          {groups.map((group) => {
            const ids = group.map((e) => e.entryId);
            const allOn = ids.every((id) => selected.has(id));
            return (
              <fieldset
                key={group[0].projectId}
                className="min-w-0 overflow-hidden rounded-lg border border-line"
              >
                <legend className="sr-only">{group[0].projectName}</legend>
                <label className="flex items-center gap-2 border-line border-b bg-surface px-3 py-2 font-semibold text-fg-strong">
                  <input
                    type="checkbox"
                    className="accent-accent"
                    checked={allOn}
                    onChange={(event) => toggle(ids, event.target.checked)}
                    aria-label={`Select all ${group[0].projectName} time`}
                  />
                  <span className="min-w-0 flex-1 truncate">
                    {group[0].projectName}
                  </span>
                  <span className="font-normal text-fg-soft tabular-nums">
                    {formatQuantity(
                      group.reduce((s, e) => s + e.quantityHundredths, 0),
                    )}{" "}
                    h
                  </span>
                </label>
                <ul>
                  {group.map((entry) => (
                    <li
                      key={entry.entryId}
                      className="border-line/60 border-b last:border-b-0"
                    >
                      <label className="flex items-center gap-2 px-3 py-1.5">
                        <input
                          type="checkbox"
                          className="accent-accent"
                          checked={selected.has(entry.entryId)}
                          onChange={(event) =>
                            toggle([entry.entryId], event.target.checked)
                          }
                          aria-label={`${new Date(entry.startedAt).toLocaleDateString()} ${entry.description}`}
                        />
                        <span className="w-16 shrink-0 text-fg-soft">
                          {new Date(entry.startedAt).toLocaleDateString(
                            "en-US",
                            { month: "short", day: "numeric" },
                          )}
                        </span>
                        <span className="min-w-0 flex-1 truncate text-fg">
                          {entry.description}
                        </span>
                        <span className="w-14 shrink-0 text-right text-fg-soft tabular-nums">
                          {formatQuantity(entry.quantityHundredths)} h
                        </span>
                        <span className="w-24 shrink-0 text-right tabular-nums">
                          {entry.amountCents == null ? (
                            <span className="text-danger">No rate</span>
                          ) : (
                            formatUsd(entry.amountCents)
                          )}
                        </span>
                      </label>
                    </li>
                  ))}
                </ul>
              </fieldset>
            );
          })}
        </div>
      )}
      <p className="text-[11.5px] text-fg-soft" role="status">
        {chosen.length} selected · {formatQuantity(hundredths)} h ·{" "}
        {formatUsd(amount)}
        {unrated > 0 &&
          ` · ${unrated} without a rate (set one on the project or client, or type it on the line)`}
      </p>
    </Sheet>
  );
}
