import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import {
  BUTTON_PRIMARY,
  BUTTON_SECONDARY,
  FIELD,
  InlineError,
  SkeletonRows,
  TEXTAREA_FIELD,
} from "../../components/Page";
import { PdfViewer } from "../../components/PdfViewer";
import { Picker } from "../../components/Picker";
import { Field } from "../../components/Sheet";
import type { Catalog } from "../../hooks/useCatalog";
import { useInvoicePreview } from "../../hooks/useInvoicePreview";
import { useMediaQuery } from "../../hooks/useMediaQuery";
import * as api from "../../lib/api";
import { assignableClients } from "../../lib/clients";
import { addDays, localDateString, startOfDay } from "../../lib/dates";
import {
  addDaysTo,
  type DraftForm,
  type DraftLine,
  formatDate,
  formatUsd,
  fromProfile,
  lineToForm,
  newLineKey,
  quantityInput,
  rateInput,
  sameFrom,
  TERMS_OPTIONS,
  toDraftInput,
} from "../../lib/invoices";
import type { BillableEntry, InvoiceProfile } from "../../lib/types";
import { EditorSection } from "./EditorSection";
import { LineItems } from "./LineItems";
import { ProfileSheet } from "./ProfileSheet";
import { TimePicker } from "./TimePicker";

interface InvoiceEditorProps {
  /** An existing draft to edit; omit for a new invoice. */
  invoiceId?: string;
  /** Start a new invoice for a client, optionally filled from one project. */
  compose?: { clientId?: string; projectId?: string };
  catalog: Catalog;
  /** The draft was stored (first save or a later one). */
  onSaved: (id: string) => void;
  /** The draft became a numbered invoice. */
  onFinalized: (id: string) => void;
  /** Leave the editor without an invoice to show (cancel, delete). */
  onExit: () => void;
}

type Confirm = "discard" | "delete" | "finalize" | null;

function entryLine(entry: BillableEntry): DraftLine {
  const date = new Date(entry.startedAt).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
  return {
    key: newLineKey(),
    kind: "time",
    entryId: entry.entryId,
    description: entry.description,
    quantity: quantityInput(entry.quantityHundredths),
    unit: "hrs",
    rate: entry.rateCents == null ? "" : rateInput(entry.rateCents),
    detail: `${entry.projectName} · ${date}`,
    trackedHundredths: entry.quantityHundredths,
  };
}

/** Everything that would be stored, minus React keys, to spot unsaved edits. */
function snapshot(form: DraftForm): string {
  return JSON.stringify({
    ...form,
    lines: form.lines.map(({ key: _key, detail: _detail, ...rest }) => rest),
  });
}

const tomorrow = (): number => addDays(startOfDay(new Date()), 1).getTime();

/**
 * Create or edit a draft: a left column of Rise-style sections and, beside it,
 * the real invoice PDF re-rendered by Rust a moment after each change. Nothing
 * is stored until Save draft; Finalize stores, numbers and locks it.
 */
export function InvoiceEditor({
  invoiceId,
  compose,
  catalog,
  onSaved,
  onFinalized,
  onExit,
}: InvoiceEditorProps) {
  const wide = useMediaQuery("(min-width: 1000px)");
  const [form, setForm] = useState<DraftForm | null>(null);
  const [savedKey, setSavedKey] = useState<string | null>(null);
  const [profile, setProfile] = useState<InvoiceProfile | null>(null);
  const [ready, setReady] = useState<BillableEntry[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<{ text: string; path?: string } | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [picking, setPicking] = useState(false);
  const [editingProfile, setEditingProfile] = useState(false);
  const [pane, setPane] = useState<"edit" | "preview">("edit");

  // Load once: an existing draft, or the defaults for a new one (and, when
  // started from a project, its ready time as lines). Props are read at mount;
  // later changes (the route moving to this draft's id after its first save)
  // must not reset the user's edits.
  const initial = useRef({ invoiceId, compose });
  const started = useRef(false);
  const wantedClient = initial.current.compose?.clientId;
  // The catalog loads alongside the page; a client named by the route must be
  // in it before the draft can be pre-filled from that client.
  const clientReady =
    wantedClient === undefined ||
    catalog.clientById.has(wantedClient) ||
    catalog.clients.length > 0;
  const { clientById } = catalog;
  useEffect(() => {
    if (started.current || !clientReady) return;
    started.current = true;
    const { invoiceId: existing, compose: start } = initial.current;
    (async () => {
      try {
        const loadedProfile = await api.getInvoiceProfile();
        setProfile(loadedProfile);
        if (existing) {
          const invoice = await api.getInvoice(existing);
          const loaded: DraftForm = {
            id: invoice.id,
            clientId: invoice.clientId,
            billToName: invoice.clientName,
            billToAddress: invoice.billToAddress ?? "",
            billToEmail: invoice.billToEmail ?? "",
            fromName: invoice.fromName,
            fromAddress: invoice.fromAddress,
            fromEmail: invoice.fromEmail ?? "",
            fromPhone: invoice.fromPhone ?? "",
            issueDate: invoice.issueDate ?? localDateString(new Date()),
            termsDays: invoice.termsDays ?? loadedProfile.defaultTermsDays,
            subject: invoice.subject ?? "",
            notes: invoice.notes ?? "",
            paymentInstructions: invoice.paymentInstructions ?? "",
            lines: invoice.lines.map(lineToForm),
          };
          setForm(loaded);
          setSavedKey(snapshot(loaded));
          return;
        }
        const client = start?.clientId
          ? clientById.get(start.clientId)
          : undefined;
        const fresh: DraftForm = {
          clientId: client?.id ?? "",
          billToName: client?.name ?? "",
          billToAddress: client?.address ?? "",
          billToEmail: client?.email ?? "",
          ...fromProfile(loadedProfile),
          issueDate: localDateString(new Date()),
          termsDays: loadedProfile.defaultTermsDays,
          subject: "",
          notes: loadedProfile.defaultNotes ?? "",
          paymentInstructions: loadedProfile.paymentInstructions ?? "",
          lines: [],
        };
        if (client) {
          const found = await api.listBillableEntries(client.id, 0, tomorrow());
          fresh.lines = found
            .filter((e) => !start?.projectId || e.projectId === start.projectId)
            .map(entryLine);
        }
        setForm(fresh);
        setSavedKey(snapshot(fresh));
      } catch (cause) {
        setError(api.describeError(cause));
      }
    })();
  }, [clientReady, clientById]);

  const clientId = form?.clientId ?? "";
  const draftId = form?.id;
  const lineEntryIds = useMemo(
    () =>
      new Set(
        (form?.lines ?? []).flatMap((line) =>
          line.entryId ? [line.entryId] : [],
        ),
      ),
    [form?.lines],
  );

  // Time waiting to be invoiced for this client that is not on the draft yet.
  useEffect(() => {
    if (!clientId) {
      setReady([]);
      return;
    }
    let cancelled = false;
    api
      .listBillableEntries(clientId, 0, tomorrow(), draftId)
      .then((found) => {
        if (!cancelled) {
          setReady(found.filter((e) => !lineEntryIds.has(e.entryId)));
        }
      })
      .catch(() => {
        if (!cancelled) setReady([]);
      });
    return () => {
      cancelled = true;
    };
  }, [clientId, draftId, lineEntryIds]);

  const input = useMemo(
    () =>
      form?.clientId && form.lines.length > 0 && form.billToName.trim()
        ? toDraftInput(form)
        : null,
    [form],
  );
  const preview = useInvoicePreview(input);

  const dirty = form !== null && snapshot(form) !== savedKey;

  const update = useCallback((patch: Partial<DraftForm>): void => {
    setForm((previous) => (previous ? { ...previous, ...patch } : previous));
    setNotice(null);
  }, []);

  const updateLine = (key: string, patch: Partial<DraftLine>): void =>
    update({
      lines: (form?.lines ?? []).map((line) =>
        line.key === key ? { ...line, ...patch } : line,
      ),
    });

  const selectClient = (id: string): void => {
    const client = catalog.clientById.get(id);
    if (!client || !form) return;
    update({
      clientId: id,
      billToName: client.name,
      billToAddress: client.address ?? "",
      billToEmail: client.email ?? "",
      // Tracked time belongs to one client's projects.
      lines: form.lines.filter((line) => line.kind !== "time"),
    });
  };

  const addEntries = (entries: BillableEntry[]): void =>
    update({ lines: [...(form?.lines ?? []), ...entries.map(entryLine)] });

  const addFixed = (kind: "retainer" | "manual"): void =>
    update({
      lines: [
        ...(form?.lines ?? []),
        {
          key: newLineKey(),
          kind,
          description: "",
          quantity: "1",
          unit: kind === "retainer" ? "mo" : "",
          rate: "",
        },
      ],
    });

  const run = async (action: () => Promise<void>): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(api.describeError(cause));
    } finally {
      setBusy(false);
    }
  };

  /** Stores the draft; resolves to its id. */
  const store = async (): Promise<string> => {
    if (!form || !input) throw new Error("Finish the draft before saving");
    const saved = await api.saveInvoiceDraft(input);
    setForm((previous) =>
      previous ? { ...previous, id: saved.id } : previous,
    );
    setSavedKey(snapshot({ ...form, id: saved.id }));
    return saved.id;
  };

  const save = (): Promise<void> =>
    run(async () => {
      const id = await store();
      setNotice({ text: "Draft saved." });
      onSaved(id);
    });

  const finalize = (): Promise<void> =>
    run(async () => {
      const id = await store();
      const done = await api.finalizeInvoice(id);
      onFinalized(done.id);
    });

  const exportDraft = (): Promise<void> =>
    run(async () => {
      if (!input) throw new Error("Finish the draft before exporting");
      const path = await api.exportInvoicePdf({ draft: input });
      if (path) setNotice({ text: `Draft PDF saved to ${path}`, path });
    });

  const remove = (): Promise<void> =>
    run(async () => {
      if (draftId) await api.deleteDraftInvoice(draftId);
      onExit();
    });

  const requestFinalize = (): void => {
    if (!form?.fromName.trim() || !form.fromAddress.trim()) {
      setError(
        "Add your business name and address in the From section before finalizing.",
      );
      return;
    }
    setConfirm("finalize");
  };

  // A brand-new draft that still shows the old settings picks up the edited
  // ones; a stored draft keeps the From it was saved with.
  const profileSaved = (next: InvoiceProfile): void => {
    setForm((previous) =>
      previous &&
      previous.id === undefined &&
      profile &&
      sameFrom(previous, fromProfile(profile))
        ? { ...previous, ...fromProfile(next) }
        : previous,
    );
    setProfile(next);
  };

  if (form === null) {
    return (
      <div className="p-5">
        {error ? <InlineError message={error} /> : <SkeletonRows />}
      </div>
    );
  }

  const client = catalog.clientById.get(form.clientId);
  const amounts =
    preview.quote && preview.quote.lines.length === form.lines.length
      ? preview.quote.lines.map((line) => line.amountCents)
      : null;
  const total = preview.quote?.totalCents;
  const readyTotals = {
    count: ready.length,
    hundredths: ready.reduce((sum, e) => sum + e.quantityHundredths, 0),
    cents: ready.reduce((sum, e) => sum + (e.amountCents ?? 0), 0),
  };
  const dueDate = addDaysTo(form.issueDate, form.termsDays);
  const canSave = input !== null && !busy;
  const previewHint = !form.clientId
    ? "Select a client and add line items to see the invoice."
    : form.lines.length === 0
      ? "Add a line item to see the invoice."
      : (preview.problem ?? "Preparing preview...");

  const leave = (): void => (dirty ? setConfirm("discard") : onExit());

  const editor = (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
        {error && <InlineError message={error} />}
        {notice?.path && (
          <p
            role="status"
            className="flex items-center gap-2 rounded-lg border border-line bg-panel px-3 py-2 text-[11.5px] text-fg-soft"
          >
            <span className="min-w-0 flex-1 truncate">{notice.text}</span>
            {notice.path && (
              <button
                type="button"
                className="shrink-0 text-fg underline"
                onClick={() => notice.path && void revealItemInDir(notice.path)}
              >
                Show in Finder
              </button>
            )}
          </p>
        )}

        <EditorSection title="Client" defaultOpen summary={client?.name}>
          <Field label="Client" htmlFor="invoice-client">
            <Picker
              id="invoice-client"
              ariaLabel="Client"
              value={form.clientId}
              onChange={selectClient}
              placeholder="Select a client"
              options={assignableClients(catalog.clients, form.clientId).map(
                (item) => ({ value: item.id, label: item.name }),
              )}
              variant="field"
            />
          </Field>
          {assignableClients(catalog.clients, form.clientId).length === 0 && (
            <p className="text-[11.5px] text-fg-faint">
              Add a client on the Clients page first.
            </p>
          )}
        </EditorSection>

        <EditorSection title="Bill to" summary={form.billToName || "Not set"}>
          <Field label="Name" htmlFor="bill-name">
            <input
              id="bill-name"
              className={FIELD}
              value={form.billToName}
              maxLength={120}
              onChange={(event) => update({ billToName: event.target.value })}
            />
          </Field>
          <Field label="Address" htmlFor="bill-address">
            <textarea
              id="bill-address"
              className={TEXTAREA_FIELD}
              rows={3}
              value={form.billToAddress}
              maxLength={500}
              onChange={(event) =>
                update({ billToAddress: event.target.value })
              }
            />
          </Field>
          <Field label="Email" htmlFor="bill-email">
            <input
              id="bill-email"
              type="email"
              className={FIELD}
              value={form.billToEmail}
              onChange={(event) => update({ billToEmail: event.target.value })}
            />
          </Field>
        </EditorSection>

        <EditorSection
          title="From"
          defaultOpen={!form.fromName.trim() || !form.fromAddress.trim()}
          summary={form.fromName || "Add your business details"}
        >
          <Field label="Name" htmlFor="from-name">
            <input
              id="from-name"
              className={FIELD}
              value={form.fromName}
              maxLength={120}
              onChange={(event) => update({ fromName: event.target.value })}
            />
          </Field>
          <Field label="Address" htmlFor="from-address">
            <textarea
              id="from-address"
              className={TEXTAREA_FIELD}
              rows={3}
              value={form.fromAddress}
              maxLength={500}
              onChange={(event) => update({ fromAddress: event.target.value })}
            />
          </Field>
          <Field label="Email" htmlFor="from-email">
            <input
              id="from-email"
              type="email"
              className={FIELD}
              value={form.fromEmail}
              maxLength={120}
              onChange={(event) => update({ fromEmail: event.target.value })}
            />
          </Field>
          <Field label="Phone" htmlFor="from-phone">
            <input
              id="from-phone"
              type="tel"
              className={FIELD}
              value={form.fromPhone}
              maxLength={120}
              onChange={(event) => update({ fromPhone: event.target.value })}
            />
          </Field>
          <p className="text-[11.5px] text-fg-faint">
            Filled in from Invoice settings. Changes here apply to this invoice
            only.
          </p>
          <div className="flex flex-wrap gap-2">
            {profile && !sameFrom(form, fromProfile(profile)) && (
              <button
                type="button"
                className={BUTTON_SECONDARY}
                onClick={() => update(fromProfile(profile))}
              >
                Reset to settings
              </button>
            )}
            <button
              type="button"
              className={BUTTON_SECONDARY}
              onClick={() => setEditingProfile(true)}
            >
              Edit invoice settings
            </button>
          </div>
        </EditorSection>

        <EditorSection
          title="Invoice details"
          defaultOpen
          summary={`Issued ${formatDate(form.issueDate)}`}
        >
          <Field label="Issue date" htmlFor="invoice-issue">
            <input
              id="invoice-issue"
              type="date"
              className={FIELD}
              value={form.issueDate}
              onChange={(event) =>
                event.target.value && update({ issueDate: event.target.value })
              }
            />
          </Field>
          <Field
            label="Due date"
            htmlFor="invoice-terms"
            hint={`Due ${formatDate(dueDate)}`}
          >
            <Picker<number>
              id="invoice-terms"
              ariaLabel="Payment terms"
              value={form.termsDays}
              onChange={(days) => update({ termsDays: days })}
              options={TERMS_OPTIONS.map((option) => ({
                value: option.value,
                label: option.label,
              }))}
              variant="field"
            />
          </Field>
          <Field label="Subject" htmlFor="invoice-subject">
            <input
              id="invoice-subject"
              className={FIELD}
              value={form.subject}
              maxLength={200}
              placeholder="Optional, e.g. Website redesign"
              onChange={(event) => update({ subject: event.target.value })}
            />
          </Field>
        </EditorSection>

        <EditorSection
          title={`Line items (${form.lines.length})`}
          defaultOpen
          summary={total === undefined ? undefined : formatUsd(total)}
        >
          <LineItems
            lines={form.lines}
            amounts={amounts}
            total={amounts ? total : undefined}
            onChange={updateLine}
            onRemove={(key) =>
              update({ lines: form.lines.filter((line) => line.key !== key) })
            }
            onAddTime={() => setPicking(true)}
            onAddFixed={addFixed}
            ready={form.clientId ? readyTotals : null}
            onAddAllReady={() => addEntries(ready)}
            disabled={!form.clientId}
          />
        </EditorSection>

        <EditorSection
          title="Payment and notes"
          summary={form.paymentInstructions || form.notes || "None"}
        >
          <Field label="Payment instructions" htmlFor="invoice-payment">
            <textarea
              id="invoice-payment"
              className={TEXTAREA_FIELD}
              rows={3}
              value={form.paymentInstructions}
              maxLength={2000}
              onChange={(event) =>
                update({ paymentInstructions: event.target.value })
              }
            />
          </Field>
          <Field label="Notes" htmlFor="invoice-notes">
            <textarea
              id="invoice-notes"
              className={TEXTAREA_FIELD}
              rows={3}
              value={form.notes}
              maxLength={2000}
              onChange={(event) => update({ notes: event.target.value })}
            />
          </Field>
        </EditorSection>
      </div>

      <div className="flex shrink-0 flex-wrap items-center gap-2 border-line border-t bg-panel px-4 py-3">
        <span className="flex-1" />
        <button type="button" className={BUTTON_SECONDARY} onClick={leave}>
          Cancel
        </button>
        <button
          type="button"
          className={BUTTON_SECONDARY}
          disabled={!canSave || (!dirty && draftId !== undefined)}
          onClick={() => void save()}
        >
          {busy ? "Working..." : "Save draft"}
        </button>
        <button
          type="button"
          className={BUTTON_PRIMARY}
          disabled={!canSave}
          onClick={requestFinalize}
        >
          Finalize invoice
        </button>
      </div>
    </div>
  );

  const paper = (
    <PdfViewer
      bytes={preview.pdf}
      stale={preview.pending}
      label="Invoice preview"
      placeholder={previewHint}
    />
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-11 shrink-0 items-center gap-3 border-line border-b px-4">
        <button
          type="button"
          aria-label="Back to invoices"
          className="rounded p-1 text-fg-soft hover:bg-surface hover:text-fg"
          onClick={leave}
        >
          ←
        </button>
        <h2 className="font-semibold text-[13px] text-fg-strong">
          {draftId ? "Edit draft invoice" : "Create draft invoice"}
        </h2>
        {(dirty || (notice && !notice.path)) && (
          <span role="status" className="text-[11px] text-fg-faint">
            {dirty ? "Unsaved changes" : notice?.text}
          </span>
        )}
        <span className="flex-1" />
        {draftId && (
          <button
            type="button"
            className="rounded-md border border-danger/40 px-3 py-1 font-medium text-[12px] text-danger hover:bg-danger-soft disabled:opacity-40"
            disabled={busy}
            onClick={() => setConfirm("delete")}
          >
            Delete draft
          </button>
        )}
        <button
          type="button"
          className="text-[12px] text-fg-soft underline hover:text-fg disabled:opacity-40"
          disabled={!canSave}
          onClick={() => void exportDraft()}
        >
          Export draft PDF
        </button>
        {!wide && (
          <div className="ml-auto flex gap-1 text-[12px]">
            {(["edit", "preview"] as const).map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={pane === value}
                onClick={() => setPane(value)}
                className={`rounded-md px-2.5 py-1 ${pane === value ? "bg-accent-soft text-accent" : "text-fg-soft hover:bg-surface"}`}
              >
                {value === "edit" ? "Edit" : "Preview"}
              </button>
            ))}
          </div>
        )}
      </div>
      {wide ? (
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(380px,2fr)_minmax(0,3fr)]">
          <div className="min-h-0 border-line border-r">{editor}</div>
          <div className="min-h-0">{paper}</div>
        </div>
      ) : (
        <div className="min-h-0 flex-1">{pane === "edit" ? editor : paper}</div>
      )}

      {picking && form.clientId && (
        <TimePicker
          clientId={form.clientId}
          invoiceId={draftId}
          excluded={lineEntryIds}
          projectId={compose?.projectId}
          onAdd={addEntries}
          onClose={() => setPicking(false)}
        />
      )}
      {editingProfile && (
        <ProfileSheet
          onClose={() => setEditingProfile(false)}
          onSaved={profileSaved}
        />
      )}
      {confirm === "discard" && (
        <ConfirmDialog
          title="Discard changes?"
          body="Your unsaved edits to this invoice will be lost."
          confirmLabel="Discard"
          onConfirm={onExit}
          onCancel={() => setConfirm(null)}
        />
      )}
      {confirm === "delete" && (
        <ConfirmDialog
          title="Delete this draft?"
          body="The draft is removed and its tracked time is released so it can be invoiced again."
          confirmLabel="Delete draft"
          onConfirm={() => {
            setConfirm(null);
            void remove();
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
      {confirm === "finalize" && (
        <ConfirmDialog
          title="Finalize this invoice?"
          body={`It gets the next invoice number and is locked${total === undefined ? "" : ` at ${formatUsd(total)}`}. A finalized invoice can be marked paid or voided, but not edited.`}
          confirmLabel="Finalize"
          tone="affirmative"
          onConfirm={() => {
            setConfirm(null);
            void finalize();
          }}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  );
}
