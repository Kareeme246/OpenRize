import { useCallback, useEffect, useRef, useState } from "react";
import { FIELD, TEXTAREA_FIELD } from "../../components/Page";
import { Picker } from "../../components/Picker";
import { Field, Sheet } from "../../components/Sheet";
import * as api from "../../lib/api";
import { TERMS_OPTIONS } from "../../lib/invoices";
import type { InvoiceProfile } from "../../lib/types";

const MAX_LOGO_BYTES = 5 * 1024 * 1024;

interface ProfileSheetProps {
  onClose: () => void;
  onSaved: (profile: InvoiceProfile) => void;
}

/**
 * The business every invoice is "From": name, address, contact details,
 * payment instructions, logo and numbering. Stored locally; finalized invoices
 * keep their own copy, so editing this never rewrites an issued document.
 */
export function ProfileSheet({ onClose, onSaved }: ProfileSheetProps) {
  const [profile, setProfile] = useState<InvoiceProfile | null>(null);
  const [name, setName] = useState("");
  const [address, setAddress] = useState("");
  const [email, setEmail] = useState("");
  const [phone, setPhone] = useState("");
  const [payment, setPayment] = useState("");
  const [notes, setNotes] = useState("");
  const [terms, setTerms] = useState(30);
  const [nextNumber, setNextNumber] = useState("");
  const [logoUrl, setLogoUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);

  const loadLogo = useCallback(async (): Promise<void> => {
    const bytes = await api.getInvoiceLogo();
    setLogoUrl((previous) => {
      if (previous) URL.revokeObjectURL(previous);
      if (bytes.length === 0) return null;
      return URL.createObjectURL(
        new Blob([bytes.slice().buffer], { type: "image/png" }),
      );
    });
  }, []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const loaded = await api.getInvoiceProfile();
        if (cancelled) return;
        setProfile(loaded);
        setName(loaded.name);
        setAddress(loaded.address);
        setEmail(loaded.email ?? "");
        setPhone(loaded.phone ?? "");
        setPayment(loaded.paymentInstructions ?? "");
        setNotes(loaded.defaultNotes ?? "");
        setTerms(loaded.defaultTermsDays);
        setNextNumber(String(loaded.nextNumber));
        if (loaded.hasLogo) await loadLogo();
      } catch (cause) {
        if (!cancelled) setError(api.describeError(cause));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [loadLogo]);

  useEffect(
    () => () => {
      if (logoUrl) URL.revokeObjectURL(logoUrl);
    },
    [logoUrl],
  );

  const chooseLogo = async (file: File | undefined): Promise<void> => {
    if (!file) return;
    setError(null);
    if (file.size > MAX_LOGO_BYTES) {
      setError("Logos can be at most 5 MB.");
      return;
    }
    try {
      await api.setInvoiceLogo(new Uint8Array(await file.arrayBuffer()));
      await loadLogo();
    } catch (cause) {
      setError(api.describeError(cause));
    }
  };

  const removeLogo = async (): Promise<void> => {
    try {
      await api.clearInvoiceLogo();
      await loadLogo();
    } catch (cause) {
      setError(api.describeError(cause));
    }
  };

  const number = Number(nextNumber);
  const numberValid =
    Number.isInteger(number) && number >= 1 && number <= 99_999;

  const submit = async (): Promise<void> => {
    setBusy(true);
    setError(null);
    try {
      const saved = await api.updateInvoiceProfile({
        name,
        address,
        email,
        phone,
        paymentInstructions: payment,
        defaultNotes: notes,
        defaultTermsDays: terms,
        nextNumber:
          profile && number !== profile.nextNumber ? number : undefined,
      });
      onSaved(saved);
      onClose();
    } catch (cause) {
      setError(api.describeError(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet
      title="Invoice settings"
      submitLabel={busy ? "Saving..." : "Save"}
      onSubmit={() => void submit()}
      onClose={onClose}
      canSubmit={profile !== null && !busy && numberValid}
      error={error}
      width="lg"
    >
      <p className="text-[11.5px] text-fg-faint">
        Your business details appear under From on every invoice. Issued
        invoices keep the details they were issued with.
      </p>
      <Field label="Business name" htmlFor="profile-name">
        <input
          id="profile-name"
          className={FIELD}
          value={name}
          maxLength={120}
          onChange={(event) => setName(event.target.value)}
        />
      </Field>
      <Field label="Address" htmlFor="profile-address">
        <textarea
          id="profile-address"
          className={TEXTAREA_FIELD}
          rows={3}
          value={address}
          maxLength={500}
          onChange={(event) => setAddress(event.target.value)}
        />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Email" htmlFor="profile-email">
          <input
            id="profile-email"
            type="email"
            className={FIELD}
            value={email}
            onChange={(event) => setEmail(event.target.value)}
          />
        </Field>
        <Field label="Phone" htmlFor="profile-phone">
          <input
            id="profile-phone"
            className={FIELD}
            value={phone}
            onChange={(event) => setPhone(event.target.value)}
          />
        </Field>
      </div>
      <Field
        label="Logo"
        hint="PNG or JPEG. It is resized and stored on this Mac."
      >
        <div className="flex items-center gap-3">
          <div className="flex h-14 w-40 items-center justify-center overflow-hidden rounded-md border border-line bg-white">
            {logoUrl ? (
              <img
                src={logoUrl}
                alt="Current logo"
                className="max-h-full max-w-full object-contain"
              />
            ) : (
              <span className="text-[11px] text-neutral-400">No logo</span>
            )}
          </div>
          <input
            ref={fileInput}
            type="file"
            accept="image/png,image/jpeg"
            className="sr-only"
            aria-label="Choose logo file"
            onChange={(event) => {
              void chooseLogo(event.target.files?.[0]);
              event.target.value = "";
            }}
          />
          <button
            type="button"
            className="rounded-md border border-line bg-panel px-3 py-1.5 font-medium text-[12px] text-fg-soft hover:bg-surface hover:text-fg"
            onClick={() => fileInput.current?.click()}
          >
            {logoUrl ? "Replace logo" : "Choose logo"}
          </button>
          {logoUrl && (
            <button
              type="button"
              className="text-[12px] text-fg-soft underline hover:text-fg"
              onClick={() => void removeLogo()}
            >
              Remove
            </button>
          )}
        </div>
      </Field>
      <Field
        label="Default payment instructions"
        htmlFor="profile-payment"
        hint="Printed at the bottom of new invoices, for example bank or Zelle details."
      >
        <textarea
          id="profile-payment"
          className={TEXTAREA_FIELD}
          rows={3}
          value={payment}
          maxLength={2000}
          onChange={(event) => setPayment(event.target.value)}
        />
      </Field>
      <Field label="Default notes" htmlFor="profile-notes">
        <textarea
          id="profile-notes"
          className={TEXTAREA_FIELD}
          rows={2}
          value={notes}
          maxLength={2000}
          onChange={(event) => setNotes(event.target.value)}
        />
      </Field>
      <div className="grid grid-cols-2 gap-3">
        <Field label="Default payment terms" htmlFor="profile-terms">
          <Picker<number>
            id="profile-terms"
            ariaLabel="Default payment terms"
            value={terms}
            onChange={setTerms}
            options={TERMS_OPTIONS.map((option) => ({
              value: option.value,
              label: option.label,
            }))}
            variant="field"
          />
        </Field>
        <Field
          label={`Next invoice number (${profile?.numberYear ?? ""})`}
          htmlFor="profile-next"
          hint={
            numberValid
              ? `INV-${profile?.numberYear}-${String(number).padStart(4, "0")}`
              : "Enter a number from 1 to 99999"
          }
        >
          <input
            id="profile-next"
            inputMode="numeric"
            className={FIELD}
            value={nextNumber}
            onChange={(event) =>
              setNextNumber(event.target.value.replace(/\D/g, ""))
            }
          />
        </Field>
      </div>
    </Sheet>
  );
}
