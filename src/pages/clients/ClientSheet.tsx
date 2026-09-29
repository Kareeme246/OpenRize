import { useState } from "react";
import { FIELD, TEXTAREA_FIELD } from "../../components/Page";
import { Field, Sheet } from "../../components/Sheet";
import * as api from "../../lib/api";
import { describeError } from "../../lib/api";
import type { Client } from "../../lib/types";

interface ClientSheetProps {
  /** Editing this client; absent creates one. */
  client?: Client;
  onClose: () => void;
  onSaved: (client: Client) => void;
}

/** ISO 4217 codes are three letters; an empty one means USD everywhere. */
const CURRENCY = /^[A-Za-z]{3}$/;

/**
 * New or edit client: name, billing email and address, the default rate and
 * currency its projects fall back to, and a note.
 */
export function ClientSheet({ client, onClose, onSaved }: ClientSheetProps) {
  const [name, setName] = useState(client?.name ?? "");
  const [email, setEmail] = useState(client?.email ?? "");
  const [address, setAddress] = useState(client?.address ?? "");
  const [rate, setRate] = useState(
    client?.defaultRate !== undefined && client?.defaultRate !== null
      ? String(client.defaultRate)
      : "",
  );
  const [currency, setCurrency] = useState(client?.currency ?? "");
  const [notes, setNotes] = useState(client?.notes ?? "");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const valid =
    name.trim() !== "" &&
    (rate === "" || Number(rate) >= 0) &&
    (currency.trim() === "" || CURRENCY.test(currency.trim()));

  const submit = async (): Promise<void> => {
    setSaving(true);
    try {
      const fields = {
        name: name.trim(),
        email: email.trim() || null,
        address: address.trim() || null,
        defaultRate: rate === "" ? null : Number(rate),
        currency: currency.trim().toUpperCase() || null,
        notes: notes.trim() || null,
      };
      const saved = client
        ? await api.updateClient(client.id, fields)
        : await api.createClient({
            name: fields.name,
            email: fields.email ?? undefined,
            address: fields.address ?? undefined,
            defaultRate: fields.defaultRate ?? undefined,
            currency: fields.currency ?? undefined,
            notes: fields.notes ?? undefined,
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
      title={client ? `Edit ${client.name}` : "New client"}
      submitLabel={saving ? "Saving…" : client ? "Save" : "Create client"}
      canSubmit={valid && !saving}
      onSubmit={submit}
      onClose={onClose}
      error={error}
      width="md"
    >
      <Field label="Name" htmlFor="client-name">
        <input
          id="client-name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          // biome-ignore lint/a11y/noAutofocus: the sheet opens to type a name; otherwise the close button takes focus.
          autoFocus
          placeholder="e.g. Acme Corp"
          className={FIELD}
          required
        />
      </Field>

      <Field label="Billing email" htmlFor="client-email">
        <input
          id="client-email"
          type="email"
          value={email}
          onChange={(event) => setEmail(event.target.value)}
          placeholder="billing@example.com"
          className={FIELD}
        />
      </Field>

      <Field label="Address" htmlFor="client-address">
        <textarea
          id="client-address"
          value={address}
          onChange={(event) => setAddress(event.target.value)}
          placeholder={"1 Main St.\nSpringfield"}
          rows={3}
          className={TEXTAREA_FIELD}
        />
      </Field>

      <div className="grid grid-cols-[minmax(0,1fr)_120px] gap-3">
        <Field
          label="Default hourly rate"
          htmlFor="client-rate"
          hint="Projects with no rate of their own bill at this"
        >
          <input
            id="client-rate"
            type="number"
            min="0"
            step="any"
            value={rate}
            onChange={(event) => setRate(event.target.value)}
            placeholder="None"
            className={FIELD}
          />
        </Field>
        <Field label="Currency" htmlFor="client-currency">
          <input
            id="client-currency"
            value={currency}
            maxLength={3}
            onChange={(event) => setCurrency(event.target.value)}
            placeholder="USD"
            className={`${FIELD} uppercase`}
          />
        </Field>
      </div>

      <Field label="Notes" htmlFor="client-notes">
        <textarea
          id="client-notes"
          value={notes}
          onChange={(event) => setNotes(event.target.value)}
          placeholder="Payment terms, contacts, anything worth remembering"
          rows={3}
          className={TEXTAREA_FIELD}
        />
      </Field>
    </Sheet>
  );
}
