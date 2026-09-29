import { useCallback, useEffect, useState } from "react";
import { InlineError, PageHeader } from "../components/Page";
import { useCatalog } from "../hooks/useCatalog";
import { useTauriEvent } from "../hooks/useTauriEvent";
import * as api from "../lib/api";
import type { InvoiceSummary, Route } from "../lib/types";
import { InvoiceEditor } from "./invoices/InvoiceEditor";
import { InvoiceList } from "./invoices/InvoiceList";
import { InvoiceView } from "./invoices/InvoiceView";
import { ProfileSheet } from "./invoices/ProfileSheet";

type InvoicesRoute = Extract<Route, { name: "invoices" }>;

interface InvoicesProps {
  route: InvoicesRoute;
  navigate: (route: Route) => void;
  replace: (route: Route) => void;
}

/**
 * Invoices: the list, and from it a draft's editor (with the live PDF) or a
 * finalized invoice's document. The route says which; a draft opens the
 * editor, anything else opens the archived document.
 */
export function Invoices({ route, navigate, replace }: InvoicesProps) {
  const catalog = useCatalog();
  const [invoices, setInvoices] = useState<InvoiceSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [settings, setSettings] = useState(false);

  const refresh = useCallback(async (): Promise<void> => {
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

  const list = (): void => navigate({ name: "invoices" });
  const summary = route.invoiceId
    ? invoices.find((invoice) => invoice.id === route.invoiceId)
    : undefined;
  const editing =
    route.compose !== undefined ||
    (summary?.status === "draft" && !summary.legacy);

  let body = null;
  if (editing) {
    // One editor instance from "new" through its first save and finalize.
    body = (
      <InvoiceEditor
        key="editor"
        invoiceId={route.invoiceId}
        compose={route.compose}
        catalog={catalog}
        onSaved={async (id) => {
          await refresh();
          replace({ name: "invoices", invoiceId: id });
        }}
        onFinalized={async (id) => {
          await refresh();
          replace({ name: "invoices", invoiceId: id });
        }}
        onExit={async () => {
          await refresh();
          list();
        }}
      />
    );
  } else if (route.invoiceId) {
    body = loading ? null : summary ? (
      <InvoiceView
        key={route.invoiceId}
        invoiceId={route.invoiceId}
        onBack={list}
        onChanged={() => void refresh()}
      />
    ) : (
      <div className="p-5">
        <InlineError message="That invoice no longer exists." onRetry={list} />
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-base">
      <PageHeader title="Invoices" />
      {(error || catalog.error) && (
        <div className="px-5 pt-4">
          <InlineError
            message={error || catalog.error || ""}
            onRetry={() => {
              void refresh();
              void catalog.reload();
            }}
          />
        </div>
      )}
      {body ?? (
        <InvoiceList
          invoices={invoices}
          loading={loading}
          onOpen={(id) => navigate({ name: "invoices", invoiceId: id })}
          onCreate={() => navigate({ name: "invoices", compose: {} })}
          onSettings={() => setSettings(true)}
        />
      )}
      {settings && (
        <ProfileSheet onClose={() => setSettings(false)} onSaved={() => {}} />
      )}
    </div>
  );
}
