import { addDays, localDateString, parseLocalDate } from "./dates";
import type {
  InvoiceDraftInput,
  InvoiceLine,
  InvoiceLineInput,
  InvoiceLineKind,
  InvoiceProfile,
  InvoiceSummary,
} from "./types";

/*
 * Invoice helpers. All money is integer USD cents and all quantities integer
 * hundredths; nothing here uses floating point for amounts. Rust prices every
 * line (see src-tauri/src/invoices/money.rs): this file only parses what the
 * user types, formats what Rust returns, and shapes the editor's state into a
 * draft.
 */

const USD = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
});

/** `$1,234.56`. */
export function formatUsd(cents: number): string {
  return USD.format(cents / 100);
}

/** `1.50` for fractions, `3` for whole numbers. */
export function formatQuantity(hundredths: number): string {
  const whole = Math.trunc(hundredths / 100);
  const fraction = hundredths % 100;
  return fraction === 0
    ? String(whole)
    : `${whole}.${String(fraction).padStart(2, "0")}`;
}

/** A quantity as an editable string: `1.5`, `2`, `0.25`. */
export function quantityInput(hundredths: number): string {
  return formatQuantity(hundredths).replace(/(\.\d)0$/, "$1");
}

/** A rate as an editable string: `120`, `120.50`. */
export function rateInput(cents: number): string {
  const whole = Math.trunc(cents / 100);
  const fraction = cents % 100;
  return fraction === 0
    ? String(whole)
    : `${whole}.${String(fraction).padStart(2, "0")}`;
}

const DECIMAL = /^(\d{1,9})(?:\.(\d{0,2}))?$/;

/** `"1.5"` to 150; null for blanks, zero, negatives and 3+ decimals. */
export function parseQuantity(text: string): number | null {
  const match = DECIMAL.exec(text.trim());
  if (!match) return null;
  const hundredths =
    Number(match[1]) * 100 + Number((match[2] ?? "").padEnd(2, "0") || 0);
  return hundredths > 0 && hundredths <= 1_000_000_000 ? hundredths : null;
}

/** `"$1,200.50"` to 120050; null for blanks, negatives and 3+ decimals. */
export function parseRateCents(text: string): number | null {
  const match = DECIMAL.exec(text.trim().replace(/^\$/, "").replace(/,/g, ""));
  if (!match) return null;
  const cents =
    Number(match[1]) * 100 + Number((match[2] ?? "").padEnd(2, "0") || 0);
  return cents <= 1_000_000_000 ? cents : null;
}

/** Net terms offered on an invoice; 0 is "Due on receipt". */
export const TERMS_OPTIONS = [
  { value: 0, label: "Due on receipt" },
  { value: 15, label: "Net 15" },
  { value: 30, label: "Net 30" },
  { value: 45, label: "Net 45" },
  { value: 60, label: "Net 60" },
  { value: 90, label: "Net 90" },
] as const;

export function termsLabel(days: number): string {
  return days === 0 ? "Due on receipt" : `Net ${days}`;
}

/** `YYYY-MM-DD` plus `days`, in local calendar days. */
export function addDaysTo(date: string, days: number): string {
  return localDateString(addDays(parseLocalDate(date), days));
}

/** `Oct 29, 2026` from a `YYYY-MM-DD`. */
export function formatDate(date?: string | null): string {
  if (!date) return "";
  return parseLocalDate(date).toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

/** The status a list shows: an open invoice past its due date is overdue. */
export type DisplayStatus = "draft" | "open" | "overdue" | "paid" | "void";

export function displayStatus(
  invoice: Pick<InvoiceSummary, "status" | "dueDate">,
  today = localDateString(new Date()),
): DisplayStatus {
  if (invoice.status === "open" && invoice.dueDate && invoice.dueDate < today) {
    return "overdue";
  }
  return invoice.status;
}

export const STATUS_LABEL: Record<DisplayStatus, string> = {
  draft: "Draft",
  open: "Open",
  overdue: "Overdue",
  paid: "Paid",
  void: "Void",
};

/** The From fields of a draft, as Invoice settings would fill them. */
export type FromFields = Pick<
  DraftForm,
  "fromName" | "fromAddress" | "fromEmail" | "fromPhone"
>;

export function fromProfile(profile: InvoiceProfile): FromFields {
  return {
    fromName: profile.name,
    fromAddress: profile.address,
    fromEmail: profile.email ?? "",
    fromPhone: profile.phone ?? "",
  };
}

export function sameFrom(a: FromFields, b: FromFields): boolean {
  return (
    a.fromName === b.fromName &&
    a.fromAddress === b.fromAddress &&
    a.fromEmail === b.fromEmail &&
    a.fromPhone === b.fromPhone
  );
}

/** One editable row of the draft; text fields hold what the user typed. */
export interface DraftLine {
  /** Stable React key; never sent. */
  key: string;
  kind: InvoiceLineKind;
  entryId?: string;
  description: string;
  /** Time lines take theirs from the entry; others are typed. */
  quantity: string;
  unit: string;
  rate: string;
  /** For time lines: `Project · Sep 12, 2026`, shown under the description. */
  detail?: string;
  /** For time lines: the hours from the tracked entry, in hundredths. */
  trackedHundredths?: number;
}

let lineCounter = 0;
export function newLineKey(): string {
  lineCounter += 1;
  return `line-${lineCounter}`;
}

export interface DraftForm {
  id?: string;
  clientId: string;
  billToName: string;
  billToAddress: string;
  billToEmail: string;
  fromName: string;
  fromAddress: string;
  fromEmail: string;
  fromPhone: string;
  issueDate: string;
  termsDays: number;
  subject: string;
  notes: string;
  paymentInstructions: string;
  lines: DraftLine[];
}

const blank = (value: string): string | undefined => {
  const trimmed = value.trim();
  return trimmed === "" ? undefined : trimmed;
};

export interface LineProblems {
  quantity?: string;
  rate?: string;
  description?: string;
}

/** What is wrong with a line as typed; empty when it can be sent. */
export function lineProblems(line: DraftLine): LineProblems {
  const problems: LineProblems = {};
  if (line.kind !== "time" && line.description.trim() === "") {
    problems.description = "Add a description";
  }
  if (line.kind !== "time" && parseQuantity(line.quantity) === null) {
    problems.quantity = "Enter a quantity above 0";
  }
  if (parseRateCents(line.rate) === null) {
    problems.rate = "Enter a rate";
  }
  return problems;
}

export function toLineInput(line: DraftLine): InvoiceLineInput {
  const rateCents = parseRateCents(line.rate) ?? undefined;
  if (line.kind === "time") {
    return {
      kind: "time",
      entryId: line.entryId,
      description: line.description,
      rateCents,
    };
  }
  return {
    kind: line.kind,
    description: line.description,
    quantityHundredths: parseQuantity(line.quantity) ?? undefined,
    unit: blank(line.unit),
    rateCents,
  };
}

/**
 * The form as a draft for Rust, or null while any line is unfinished (the
 * preview then keeps showing the last good document instead of an error).
 */
export function toDraftInput(form: DraftForm): InvoiceDraftInput | null {
  if (form.lines.some((line) => Object.keys(lineProblems(line)).length > 0)) {
    return null;
  }
  return {
    id: form.id,
    clientId: form.clientId,
    billToName: form.billToName,
    billToAddress: blank(form.billToAddress),
    billToEmail: blank(form.billToEmail),
    fromName: form.fromName,
    fromAddress: form.fromAddress,
    fromEmail: blank(form.fromEmail),
    fromPhone: blank(form.fromPhone),
    issueDate: form.issueDate,
    termsDays: form.termsDays,
    subject: blank(form.subject),
    notes: blank(form.notes),
    paymentInstructions: blank(form.paymentInstructions),
    lines: form.lines.map(toLineInput),
  };
}

/** A stored line back into an editable one. */
export function lineToForm(line: InvoiceLine): DraftLine {
  const detail =
    line.kind === "time"
      ? [
          line.projectName,
          line.startedAt
            ? new Date(line.startedAt).toLocaleDateString("en-US", {
                month: "short",
                day: "numeric",
                year: "numeric",
              })
            : "",
        ]
          .filter(Boolean)
          .join(" · ")
      : undefined;
  return {
    key: newLineKey(),
    kind: line.kind,
    entryId: line.entryId ?? undefined,
    description: line.description,
    quantity: quantityInput(line.quantityHundredths),
    unit: line.unit,
    rate: rateInput(line.rateCents),
    detail,
    trackedHundredths:
      line.kind === "time" ? line.quantityHundredths : undefined,
  };
}
