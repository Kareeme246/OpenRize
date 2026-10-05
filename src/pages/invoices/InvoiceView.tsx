import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import {
  BUTTON_PRIMARY,
  BUTTON_SECONDARY,
  InlineError,
  SkeletonRows,
} from "../../components/Page";
import { PdfViewer } from "../../components/PdfViewer";
import * as api from "../../lib/api";
import {
  displayStatus,
  formatDate,
  formatUsd,
  termsLabel,
} from "../../lib/invoices";
import { FILE_MANAGER } from "../../lib/platform";
import type { Invoice } from "../../lib/types";
import { StatusPill } from "./StatusPill";

interface InvoiceViewProps {
  invoiceId: string;
  onBack: () => void;
  /** Something changed (paid, voided): the list should refresh. */
  onChanged: () => void;
}

/**
 * A finalized invoice: the archived PDF exactly as issued, its facts, and the
 * only things that can still happen to it (export, mark paid, void).
 */
export function InvoiceView({
  invoiceId,
  onBack,
  onChanged,
}: InvoiceViewProps) {
  const [invoice, setInvoice] = useState<Invoice | null>(null);
  const [pdf, setPdf] = useState<Uint8Array | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ text: string; path?: string } | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const [confirmVoid, setConfirmVoid] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const load = useCallback(async (): Promise<void> => {
    try {
      const loaded = await api.getInvoice(invoiceId);
      setInvoice(loaded);
      setPdf(await api.getInvoicePdf(invoiceId));
      setError(null);
    } catch (cause) {
      setError(api.describeError(cause));
    }
  }, [invoiceId]);

  useEffect(() => {
    void load();
  }, [load]);

  const act = async (action: () => Promise<void>): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await action();
      onChanged();
      await load();
    } catch (cause) {
      setError(api.describeError(cause));
    } finally {
      setBusy(false);
    }
  };

  const exportPdf = async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      const path = await api.exportInvoicePdf({ id: invoiceId });
      if (path) setNotice({ text: `PDF saved to ${path}`, path });
    } catch (cause) {
      setError(api.describeError(cause));
    } finally {
      setBusy(false);
    }
  };

  const deleteVoided = async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await api.deleteVoidInvoice(invoiceId);
      onChanged();
      onBack();
    } catch (cause) {
      setError(api.describeError(cause));
      setBusy(false);
    }
  };

  if (invoice === null) {
    return (
      <div className="p-5">
        {error ? (
          <InlineError message={error} onRetry={load} />
        ) : (
          <SkeletonRows />
        )}
      </div>
    );
  }

  const shown = displayStatus(invoice);

  const facts: [string, string][] = [
    ["Bill to", invoice.clientName],
    [
      "Issued",
      invoice.issueDate
        ? formatDate(invoice.issueDate)
        : new Date(invoice.createdAt).toLocaleDateString(),
    ],
    [
      "Due",
      invoice.dueDate
        ? `${formatDate(invoice.dueDate)}${invoice.termsDays == null ? "" : ` (${termsLabel(invoice.termsDays)})`}`
        : "",
    ],
    ...(invoice.subject
      ? ([["Subject", invoice.subject]] as [string, string][])
      : []),
    ...(invoice.paidAt
      ? ([["Paid", new Date(invoice.paidAt).toLocaleDateString()]] as [
          string,
          string,
        ][])
      : []),
  ];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-11 shrink-0 items-center gap-3 border-line border-b px-4">
        <button
          type="button"
          aria-label="Back to invoices"
          className="rounded p-1 text-fg-soft hover:bg-surface hover:text-fg"
          onClick={onBack}
        >
          ←
        </button>
        <h2 className="font-semibold text-[13px] text-fg-strong">
          {invoice.number ?? "Invoice"}
        </h2>
        <StatusPill status={shown} />
        {invoice.status === "void" && (
          <button
            type="button"
            className="ml-auto rounded-md border border-danger/40 px-3 py-1 font-medium text-[12px] text-danger hover:bg-danger-soft disabled:opacity-40"
            disabled={busy}
            onClick={() => setConfirmDelete(true)}
          >
            Delete invoice
          </button>
        )}
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[300px_1fr] max-[900px]:grid-cols-1">
        <aside className="min-h-0 space-y-4 overflow-y-auto border-line border-r p-4 text-[12px]">
          {error && <InlineError message={error} />}
          {notice && (
            <p
              role="status"
              className="rounded-lg border border-line bg-panel px-3 py-2 text-[11.5px] text-fg-soft"
            >
              {notice.text}
              {notice.path && (
                <>
                  {" "}
                  <button
                    type="button"
                    className="text-fg underline"
                    onClick={() =>
                      notice.path && void revealItemInDir(notice.path)
                    }
                  >
                    Show in {FILE_MANAGER}
                  </button>
                </>
              )}
            </p>
          )}
          <div>
            <p className="text-fg-soft">Amount due</p>
            <p className="font-semibold text-[24px] text-fg-strong tabular-nums">
              {formatUsd(invoice.totalCents)}
            </p>
          </div>
          <dl className="space-y-2">
            {facts.map(([label, value]) => (
              <div key={label}>
                <dt className="text-[11px] text-fg-faint">{label}</dt>
                <dd className="text-fg">{value}</dd>
              </div>
            ))}
          </dl>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={BUTTON_PRIMARY}
              disabled={busy}
              onClick={() => void exportPdf()}
            >
              Export PDF
            </button>
            {invoice.status === "open" && (
              <button
                type="button"
                className={BUTTON_SECONDARY}
                disabled={busy}
                onClick={() =>
                  void act(() => api.setInvoicePaid(invoice.id, true))
                }
              >
                Mark paid
              </button>
            )}
            {invoice.status === "paid" && (
              <button
                type="button"
                className={BUTTON_SECONDARY}
                disabled={busy}
                onClick={() =>
                  void act(() => api.setInvoicePaid(invoice.id, false))
                }
              >
                Mark unpaid
              </button>
            )}
            {invoice.status === "open" && (
              <button
                type="button"
                className="rounded-md border border-danger/40 px-3 py-1.5 font-medium text-[12px] text-danger hover:bg-danger-soft disabled:opacity-40"
                disabled={busy}
                onClick={() => setConfirmVoid(true)}
              >
                Void
              </button>
            )}
          </div>
          <p className="text-[11px] text-fg-faint">
            OpenRize does not send invoices or collect payment: export the PDF
            and send it yourself, then mark it paid here.
          </p>
        </aside>
        <div className="min-h-0">
          <PdfViewer
            bytes={pdf}
            label={`Invoice ${invoice.number ?? ""}`}
            placeholder="Loading invoice..."
          />
        </div>
      </div>
      {confirmVoid && (
        <ConfirmDialog
          title="Void this invoice?"
          body={`${invoice.number ?? "This invoice"} stays on record as void and its number is not reused. Its tracked time is released so it can be invoiced again.`}
          confirmLabel="Void invoice"
          onConfirm={() => {
            setConfirmVoid(false);
            void act(() => api.voidInvoice(invoice.id));
          }}
          onCancel={() => setConfirmVoid(false)}
        />
      )}
      {confirmDelete && (
        <ConfirmDialog
          title="Delete this invoice?"
          body={`${invoice.number ?? "This invoice"} will be permanently removed. This cannot be undone.`}
          confirmLabel="Delete invoice"
          onConfirm={() => {
            setConfirmDelete(false);
            void deleteVoided();
          }}
          onCancel={() => setConfirmDelete(false)}
        />
      )}
    </div>
  );
}
