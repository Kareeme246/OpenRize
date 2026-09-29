import { BUTTON_SECONDARY, FIELD } from "../../components/Page";
import {
  type DraftLine,
  formatQuantity,
  formatUsd,
  lineProblems,
} from "../../lib/invoices";

interface LineItemsProps {
  lines: DraftLine[];
  /** Amounts priced by Rust, by line index; null while a render is pending. */
  amounts: number[] | null;
  /** Rust's total for the priced lines, when it is current. */
  total?: number;
  onChange: (key: string, patch: Partial<DraftLine>) => void;
  onRemove: (key: string) => void;
  onAddTime: () => void;
  onAddFixed: (kind: "retainer" | "manual") => void;
  /** Tracked time waiting to be invoiced for the chosen client. */
  ready: { count: number; hundredths: number; cents: number } | null;
  onAddAllReady: () => void;
  disabled: boolean;
}

/**
 * The invoice's rows: tracked time (one per entry), a fixed retainer fee, or a
 * manual item. Quantity times rate is priced by Rust; the amounts shown here
 * are its answer, so the editor can never disagree with the paper.
 */
export function LineItems({
  lines,
  amounts,
  total,
  onChange,
  onRemove,
  onAddTime,
  onAddFixed,
  ready,
  onAddAllReady,
  disabled,
}: LineItemsProps) {
  return (
    <div className="space-y-3">
      {ready && ready.count > 0 && (
        <div className="flex flex-wrap items-center gap-2 rounded-lg border border-accent/30 bg-accent-soft px-3 py-2 text-[11.5px] text-fg">
          <span className="min-w-0 flex-1">
            {ready.count} approved billable{" "}
            {ready.count === 1 ? "entry is" : "entries are"} ready to invoice (
            {formatQuantity(ready.hundredths)} h · {formatUsd(ready.cents)}).
          </span>
          <button
            type="button"
            className="rounded-md bg-accent px-2.5 py-1 font-semibold text-accent-fg"
            onClick={onAddAllReady}
          >
            Add all
          </button>
        </div>
      )}

      {lines.length === 0 ? (
        <p className="rounded-lg border border-line border-dashed px-3 py-4 text-center text-[11.5px] text-fg-faint">
          No line items yet. Add tracked time, a retainer, or a manual item.
        </p>
      ) : (
        <ul className="space-y-2">
          {lines.map((line, index) => (
            <LineRow
              key={line.key}
              line={line}
              amount={amounts?.[index]}
              onChange={(patch) => onChange(line.key, patch)}
              onRemove={() => onRemove(line.key)}
            />
          ))}
        </ul>
      )}

      {total !== undefined && lines.length > 0 && (
        <p className="flex justify-between border-line border-t pt-2 font-semibold text-[13px] text-fg-strong">
          <span>Amount due</span>
          <span className="tabular-nums">{formatUsd(total)}</span>
        </p>
      )}

      <div className="flex flex-wrap gap-2">
        <button
          type="button"
          className={BUTTON_SECONDARY}
          disabled={disabled}
          onClick={onAddTime}
          title={disabled ? "Select a client first" : undefined}
        >
          + Tracked time
        </button>
        <button
          type="button"
          className={BUTTON_SECONDARY}
          onClick={() => onAddFixed("retainer")}
        >
          + Retainer / fixed fee
        </button>
        <button
          type="button"
          className={BUTTON_SECONDARY}
          onClick={() => onAddFixed("manual")}
        >
          + Manual item
        </button>
      </div>
      {lines.some((line) => line.kind === "retainer") &&
        lines.some((line) => line.kind === "time") && (
          <p className="text-[11px] text-fg-faint">
            Time already covered by a retainer should not also be billed by the
            hour; remove it here to avoid billing it twice.
          </p>
        )}
    </div>
  );
}

const KIND_LABEL = {
  time: "Tracked time",
  retainer: "Retainer",
  manual: "Item",
} as const;

function LineRow({
  line,
  amount,
  onChange,
  onRemove,
}: {
  line: DraftLine;
  amount: number | undefined;
  onChange: (patch: Partial<DraftLine>) => void;
  onRemove: () => void;
}) {
  const problems = lineProblems(line);
  const tracked = line.kind === "time";
  const label = `${KIND_LABEL[line.kind]} line`;
  const invalid = "border-danger";
  const messages = [problems.description, problems.quantity, problems.rate]
    .filter(Boolean)
    .join(" · ");
  return (
    <li className="rounded-lg border border-line bg-surface p-2.5">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <input
            aria-label={`${label} description`}
            className={`${FIELD} ${problems.description ? invalid : ""}`}
            value={line.description}
            placeholder={
              line.kind === "retainer"
                ? "Monthly retainer, September 2026"
                : "Description"
            }
            maxLength={2000}
            onChange={(event) => onChange({ description: event.target.value })}
          />
          {line.detail && (
            <p className="mt-1 truncate text-[11px] text-fg-faint">
              {line.detail}
            </p>
          )}
        </div>
        <button
          type="button"
          aria-label={`Remove ${label.toLowerCase()}`}
          onClick={onRemove}
          className="rounded p-1.5 text-fg-faint hover:bg-panel hover:text-danger"
        >
          ✕
        </button>
      </div>
      {/* The inputs share whatever width the card has (the editor column can be
          narrow); only the amount and the operators keep a fixed size. */}
      <div className="mt-1.5 flex items-center gap-1.5 text-fg-soft tabular-nums">
        {tracked ? (
          <span className="shrink-0 text-fg">
            {formatQuantity(line.trackedHundredths ?? 0)} h
          </span>
        ) : (
          <>
            <div className="min-w-0 flex-[3]">
              <input
                aria-label={`${label} quantity`}
                inputMode="decimal"
                className={`${FIELD} text-right ${problems.quantity ? invalid : ""}`}
                value={line.quantity}
                onChange={(event) => onChange({ quantity: event.target.value })}
              />
            </div>
            <div className="min-w-0 flex-[4]">
              <input
                aria-label={`${label} unit`}
                className={FIELD}
                value={line.unit}
                placeholder="unit"
                maxLength={16}
                onChange={(event) => onChange({ unit: event.target.value })}
              />
            </div>
          </>
        )}
        <span aria-hidden="true" className="shrink-0">
          ×
        </span>
        <div className="relative min-w-0 flex-[5]">
          <span
            aria-hidden="true"
            className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2 text-fg-faint"
          >
            $
          </span>
          <input
            aria-label={`${label} rate`}
            inputMode="decimal"
            className={`${FIELD} pl-5 text-right ${problems.rate ? invalid : ""}`}
            value={line.rate}
            placeholder="0.00"
            onChange={(event) => onChange({ rate: event.target.value })}
          />
        </div>
        <span aria-hidden="true" className="shrink-0">
          =
        </span>
        <span className="min-w-[4.5rem] shrink-0 text-right font-semibold text-fg-strong">
          {amount === undefined ? "-" : formatUsd(amount)}
        </span>
      </div>
      {messages && (
        <p role="alert" className="mt-1.5 text-[11px] text-danger">
          {messages}
        </p>
      )}
    </li>
  );
}
