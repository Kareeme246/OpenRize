import { type MouseEvent, useMemo, useRef, useState } from "react";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import {
  BUTTON_PRIMARY,
  BUTTON_SECONDARY,
  EmptyState,
  FIELD,
  FilterSelect,
  SkeletonRows,
} from "../../components/Page";
import {
  type DisplayStatus,
  displayStatus,
  formatDate,
  formatUsd,
  STATUS_LABEL,
} from "../../lib/invoices";
import type { InvoiceSummary } from "../../lib/types";
import { StatusPill } from "./StatusPill";

interface InvoiceListProps {
  invoices: InvoiceSummary[];
  loading: boolean;
  onOpen: (id: string) => void;
  /** Deletes a draft and releases its time; the list refreshes afterwards. */
  onDeleteDraft: (id: string) => Promise<void>;
  onCreate: () => void;
  onSettings: () => void;
}

/** Sums the totals of invoices in `status`. */
function sum(list: InvoiceSummary[], wanted: DisplayStatus): number {
  return list
    .filter((invoice) => displayStatus(invoice) === wanted)
    .reduce((total, invoice) => total + invoice.totalCents, 0);
}

export function InvoiceList({
  invoices,
  loading,
  onOpen,
  onDeleteDraft,
  onCreate,
  onSettings,
}: InvoiceListProps) {
  const [search, setSearch] = useState("");
  const [status, setStatus] = useState("");
  const [deleting, setDeleting] = useState<InvoiceSummary | null>(null);
  const pressed = useRef<{ x: number; y: number } | null>(null);

  // A drag that ends on the row (selecting text, say) is not a click on it.
  const remember = (event: MouseEvent): void => {
    pressed.current = { x: event.clientX, y: event.clientY };
  };
  const dragged = (event: MouseEvent): boolean => {
    const start = pressed.current;
    pressed.current = null;
    return (
      start !== null &&
      Math.hypot(event.clientX - start.x, event.clientY - start.y) > 4
    );
  };

  const shown = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return invoices.filter((invoice) => {
      if (status && displayStatus(invoice) !== status) return false;
      if (!needle) return true;
      return (
        (invoice.number ?? "draft").toLowerCase().includes(needle) ||
        invoice.clientName.toLowerCase().includes(needle)
      );
    });
  }, [invoices, search, status]);

  const cells: { label: string; cents: number; tone?: string }[] = [
    { label: "Open", cents: sum(invoices, "open") },
    { label: "Overdue", cents: sum(invoices, "overdue"), tone: "text-danger" },
    { label: "Paid", cents: sum(invoices, "paid") },
  ];

  return (
    <div className="min-h-0 flex-1 overflow-auto p-5">
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <input
          type="search"
          aria-label="Search invoices"
          placeholder="Search invoices..."
          className={`${FIELD} max-w-xs`}
          value={search}
          onChange={(event) => setSearch(event.target.value)}
        />
        <FilterSelect
          label="All statuses"
          value={status}
          options={(Object.keys(STATUS_LABEL) as DisplayStatus[]).map(
            (value) => ({ value, label: STATUS_LABEL[value] }),
          )}
          onChange={setStatus}
        />
        <span className="flex-1" />
        <button type="button" className={BUTTON_SECONDARY} onClick={onSettings}>
          Invoice settings
        </button>
        <button type="button" className={BUTTON_PRIMARY} onClick={onCreate}>
          + New invoice
        </button>
      </div>

      <div className="overflow-hidden rounded-xl border border-line bg-panel">
        <dl className="grid grid-cols-3 divide-x divide-line border-line border-b">
          {cells.map((cell) => (
            <div key={cell.label} className="px-4 py-3">
              <dt className="text-[11.5px] text-fg-soft">{cell.label}</dt>
              <dd
                className={`mt-0.5 font-semibold text-[20px] tabular-nums ${cell.tone ?? "text-fg-strong"}`}
              >
                {formatUsd(cell.cents)}
              </dd>
            </div>
          ))}
        </dl>

        {loading ? (
          <div className="p-4">
            <SkeletonRows />
          </div>
        ) : invoices.length === 0 ? (
          <EmptyState
            title="No invoices yet"
            hint="Create your first invoice from tracked time, or start from scratch with a retainer or manual line."
            action={
              <button
                type="button"
                className={BUTTON_PRIMARY}
                onClick={onCreate}
              >
                + New invoice
              </button>
            }
          />
        ) : shown.length === 0 ? (
          <EmptyState title="No invoices match" />
        ) : (
          <table className="w-full text-left text-[12px]">
            <thead>
              <tr className="border-line border-b text-fg-soft">
                {["Invoice", "Client", "Issue date", "Due date"].map((head) => (
                  <th key={head} scope="col" className="px-4 py-2 font-medium">
                    {head}
                  </th>
                ))}
                <th scope="col" className="px-4 py-2 text-right font-medium">
                  Amount
                </th>
                <th scope="col" className="px-4 py-2 font-medium">
                  Status
                </th>
                <th scope="col" className="px-4 py-2">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {shown.map((invoice) => (
                // The whole row opens the invoice. Keyboard users get the same
                // through the button in the first cell; its click bubbles here.
                <tr
                  key={invoice.id}
                  className="cursor-pointer select-none border-line/60 border-b last:border-b-0 hover:bg-surface"
                  onMouseDown={remember}
                  onClick={(event) => !dragged(event) && onOpen(invoice.id)}
                >
                  <td className="px-4 py-2.5">
                    <button
                      type="button"
                      className="font-semibold text-fg-strong hover:underline"
                    >
                      {invoice.number ?? "Draft"}
                    </button>
                  </td>
                  <td className="max-w-[16rem] truncate px-4 py-2.5 text-fg">
                    {invoice.clientName}
                  </td>
                  <td className="px-4 py-2.5 text-fg-soft">
                    {invoice.issueDate
                      ? formatDate(invoice.issueDate)
                      : new Date(invoice.createdAt).toLocaleDateString(
                          "en-US",
                          { month: "short", day: "numeric", year: "numeric" },
                        )}
                  </td>
                  <td className="px-4 py-2.5 text-fg-soft">
                    {formatDate(invoice.dueDate)}
                  </td>
                  <td className="px-4 py-2.5 text-right font-medium text-fg tabular-nums">
                    {formatUsd(invoice.totalCents)}
                  </td>
                  <td className="px-4 py-2.5">
                    <StatusPill status={displayStatus(invoice)} />
                  </td>
                  <td className="px-4 py-2.5 text-right">
                    {invoice.status === "draft" && (
                      <button
                        type="button"
                        className="rounded px-1.5 py-0.5 text-danger hover:bg-danger-soft"
                        onClick={(event) => {
                          event.stopPropagation();
                          setDeleting(invoice);
                        }}
                      >
                        Delete
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      {deleting && (
        <ConfirmDialog
          title="Delete this draft?"
          body="The draft is removed and its tracked time is released so it can be invoiced again."
          confirmLabel="Delete draft"
          onConfirm={() => {
            const target = deleting;
            setDeleting(null);
            void onDeleteDraft(target.id);
          }}
          onCancel={() => setDeleting(null)}
        />
      )}
    </div>
  );
}
