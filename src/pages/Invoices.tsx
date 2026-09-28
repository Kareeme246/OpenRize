import { useCallback, useEffect, useMemo, useState } from "react";
import {
  BUTTON_PRIMARY,
  BUTTON_SECONDARY,
  EmptyState,
  InlineError,
  PageHeader,
  SkeletonRows,
} from "../components/Page";
import { Picker } from "../components/Picker";
import { useCatalog } from "../hooks/useCatalog";
import { useTauriEvent } from "../hooks/useTauriEvent";
import * as api from "../lib/api";
import { addDays, localDateString, parseLocalDate } from "../lib/dates";
import { formatDuration } from "../lib/format";
import type { Invoice, TimeEntry } from "../lib/types";

const money = (cents: number, currency: string) =>
  new Intl.NumberFormat(undefined, { style: "currency", currency }).format(
    cents / 100,
  );

export function Invoices() {
  const catalog = useCatalog();
  const [invoices, setInvoices] = useState<Invoice[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [clientId, setClientId] = useState("");
  const [from, setFrom] = useState(() =>
    localDateString(
      new Date(new Date().getFullYear(), new Date().getMonth(), 1),
    ),
  );
  const [through, setThrough] = useState(() => localDateString(new Date()));
  const [eligible, setEligible] = useState<TimeEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const start = parseLocalDate(from).getTime();
  const end = addDays(parseLocalDate(through), 1).getTime();
  const validRange = from <= through;

  const refresh = useCallback(async () => {
    try {
      setInvoices(await api.listInvoices());
      setError(null);
    } catch (cause) {
      setError(api.describeError(cause));
    } finally {
      setLoading(false);
    }
  }, []);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useTauriEvent(api.ENTRIES_CHANGED, () => void refresh());

  const loadEligible = useCallback(async () => {
    if (!clientId || !validRange) {
      setEligible([]);
      return;
    }
    try {
      const entries = await api.listTimeEntries(start, end - 1);
      setEligible(
        entries.filter((entry) => {
          const project = entry.projectId
            ? catalog.projectById.get(entry.projectId)
            : undefined;
          return (
            project?.clientId === clientId &&
            entry.startedAt >= start &&
            entry.startedAt < end &&
            entry.status === "approved" &&
            entry.billable &&
            !entry.invoiceId
          );
        }),
      );
    } catch (cause) {
      setError(api.describeError(cause));
    }
  }, [clientId, start, end, validRange, catalog.projectById]);
  useEffect(() => {
    void loadEligible();
  }, [loadEligible]);
  useTauriEvent(api.ENTRIES_CHANGED, () => void loadEligible());

  const current = useMemo(
    () => invoices.find((invoice) => invoice.id === selected) ?? invoices[0],
    [invoices, selected],
  );
  const client = catalog.clientById.get(clientId);
  const missingRate = eligible.find((entry) => {
    const project = entry.projectId
      ? catalog.projectById.get(entry.projectId)
      : undefined;
    return project?.hourlyRate == null && client?.defaultRate == null;
  });

  const act = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      await refresh();
    } catch (cause) {
      setError(api.describeError(cause));
    } finally {
      setBusy(false);
    }
  };
  const create = () =>
    void act(async () => {
      const invoice = await api.createInvoice(clientId, start, end);
      setSelected(invoice.id);
    });

  return (
    <div className="flex h-full min-h-0 flex-col bg-base">
      <PageHeader title="Invoices" />
      <div className="min-h-0 flex-1 overflow-auto p-5">
        <p className="mb-4 text-xs text-fg-soft">
          Create a draft from approved billable project time. Mark sent and paid
          when those steps happen outside OpenRize.
        </p>
        {(error || catalog.error) && (
          <div className="mb-4">
            <InlineError
              message={error || catalog.error || ""}
              onRetry={() => {
                void refresh();
                void catalog.reload();
              }}
            />
          </div>
        )}
        <div className="mb-5 rounded-xl border border-line bg-panel p-4">
          <h2 className="mb-3 text-sm font-semibold text-fg-strong">
            New invoice
          </h2>
          <div className="flex flex-wrap items-end gap-3 text-xs text-fg-soft">
            <div className="flex min-w-40 flex-1 flex-col gap-1">
              <label htmlFor="invoice-client" className="text-xs text-fg-soft">
                Client
              </label>
              <Picker
                id="invoice-client"
                ariaLabel="Client"
                value={clientId}
                onChange={setClientId}
                placeholder="Select a client"
                options={[
                  { value: "", label: "Select a client" },
                  ...catalog.clients
                    .filter((item) => !item.deletedAt)
                    .map((item) => ({
                      value: item.id,
                      label: item.name,
                    })),
                ]}
                variant="field"
              />
            </div>
            <label className="flex flex-col gap-1">
              From{" "}
              <input
                type="date"
                className="rounded-md border border-line bg-surface px-2 py-1.5 text-fg"
                value={from}
                onChange={(event) => setFrom(event.target.value)}
              />
            </label>
            <label className="flex flex-col gap-1">
              Through{" "}
              <input
                type="date"
                className="rounded-md border border-line bg-surface px-2 py-1.5 text-fg"
                value={through}
                onChange={(event) => setThrough(event.target.value)}
              />
            </label>
            <button
              type="button"
              className={BUTTON_PRIMARY}
              onClick={create}
              disabled={
                busy ||
                !clientId ||
                !validRange ||
                !eligible.length ||
                !!missingRate ||
                !client?.currency
              }
            >
              Create draft
            </button>
          </div>
          <p className="mt-3 text-[11px] text-fg-faint" role="status">
            {!validRange
              ? "End date must be on or after start date."
              : !clientId
                ? "Select a client to see available time."
                : !client?.currency
                  ? "Set the client's currency in Projects before invoicing."
                  : missingRate
                    ? "Set an hourly rate on every project or the client before invoicing."
                    : `${eligible.length} approved billable ${eligible.length === 1 ? "entry" : "entries"} available · ${formatDuration(eligible.reduce((sum, entry) => sum + entry.endedAt - entry.startedAt, 0))}`}
          </p>
        </div>
        {loading ? (
          <SkeletonRows />
        ) : invoices.length === 0 ? (
          <EmptyState
            title="No invoices yet"
            hint="Approve billable project time, then create a client draft above."
          />
        ) : (
          <div className="grid gap-4 lg:grid-cols-[minmax(190px,1fr)_minmax(0,2fr)]">
            <div className="space-y-2">
              {invoices.map((invoice) => (
                <button
                  type="button"
                  key={invoice.id}
                  onClick={() => setSelected(invoice.id)}
                  aria-pressed={invoice.id === current?.id}
                  className={`w-full rounded-lg border p-3 text-left transition-colors ${invoice.id === current?.id ? "border-accent bg-accent-soft" : "border-line bg-panel hover:bg-surface"}`}
                >
                  <span className="flex items-center justify-between gap-2">
                    <span className="truncate text-sm font-semibold text-fg">
                      {invoice.clientName}
                    </span>
                    <span className="rounded bg-surface px-1.5 py-0.5 text-[10px] font-medium text-fg-soft capitalize">
                      {invoice.status}
                    </span>
                  </span>
                  <span className="mt-1 flex justify-between text-[11px] text-fg-soft">
                    <span>
                      {new Date(invoice.createdAt).toLocaleDateString()}
                    </span>
                    <span>
                      {money(
                        invoice.lines.reduce(
                          (sum, line) => sum + line.amountCents,
                          0,
                        ),
                        invoice.currency,
                      )}
                    </span>
                  </span>
                </button>
              ))}
            </div>
            {current && (
              <section
                className="min-w-0 rounded-xl border border-line bg-panel p-5"
                aria-label={`Invoice for ${current.clientName}`}
              >
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div>
                    <h2 className="text-lg font-semibold text-fg-strong">
                      Invoice · {current.clientName}
                    </h2>
                    <p className="mt-1 text-xs text-fg-soft">
                      Created {new Date(current.createdAt).toLocaleDateString()}{" "}
                      · <span className="capitalize">{current.status}</span>
                    </p>
                    {current.clientAddress && (
                      <p className="mt-2 whitespace-pre-line text-xs text-fg-soft">
                        {current.clientAddress}
                      </p>
                    )}
                    {current.clientEmail && (
                      <p className="mt-1 text-xs text-fg-soft">
                        {current.clientEmail}
                      </p>
                    )}
                  </div>
                  <div className="flex flex-wrap gap-2">
                    {current.status === "draft" && (
                      <>
                        <button
                          type="button"
                          className={BUTTON_PRIMARY}
                          disabled={busy}
                          onClick={() =>
                            void act(() =>
                              api.setInvoiceStatus(current.id, "sent"),
                            )
                          }
                        >
                          Mark sent
                        </button>
                        <button
                          type="button"
                          className={BUTTON_SECONDARY}
                          disabled={busy}
                          onClick={() => {
                            if (
                              window.confirm(
                                "Delete this draft and release its time entries?",
                              )
                            )
                              void act(() =>
                                api.deleteDraftInvoice(current.id),
                              );
                          }}
                        >
                          Delete draft
                        </button>
                      </>
                    )}
                    {current.status === "sent" && (
                      <button
                        type="button"
                        className={BUTTON_PRIMARY}
                        disabled={busy}
                        onClick={() =>
                          void act(() =>
                            api.setInvoiceStatus(current.id, "paid"),
                          )
                        }
                      >
                        Mark paid
                      </button>
                    )}
                  </div>
                </div>
                <div className="mt-5 overflow-x-auto">
                  <table className="w-full min-w-[440px] text-left text-xs tabular-nums">
                    <thead>
                      <tr className="border-b border-line text-fg-soft">
                        <th scope="col" className="py-2">
                          Project / work
                        </th>
                        <th scope="col" className="py-2 text-right">
                          Hours
                        </th>
                        <th scope="col" className="py-2 text-right">
                          Rate
                        </th>
                        <th scope="col" className="py-2 text-right">
                          Amount
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      {current.lines.map((line) => (
                        <tr
                          key={line.entryId}
                          className="border-b border-line/60 text-fg"
                        >
                          <td className="py-2 pr-2">
                            <span className="font-medium">
                              {line.projectName}
                            </span>
                            <br />
                            <span className="text-fg-soft">
                              {new Date(line.startedAt).toLocaleDateString()} ·{" "}
                              {line.description}
                            </span>
                          </td>
                          <td className="py-2 text-right">
                            {(
                              (line.endedAt - line.startedAt) /
                              3_600_000
                            ).toFixed(2)}
                          </td>
                          <td className="py-2 text-right">
                            {money(line.rate * 100, current.currency)}
                          </td>
                          <td className="py-2 text-right">
                            {money(line.amountCents, current.currency)}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                    <tfoot>
                      <tr className="font-semibold text-fg-strong">
                        <th scope="row" colSpan={3} className="pt-3 text-right">
                          Total
                        </th>
                        <td className="pt-3 text-right">
                          {money(
                            current.lines.reduce(
                              (sum, line) => sum + line.amountCents,
                              0,
                            ),
                            current.currency,
                          )}
                        </td>
                      </tr>
                    </tfoot>
                  </table>
                </div>
              </section>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
