import { type DisplayStatus, STATUS_LABEL } from "../../lib/invoices";

const TONE: Record<DisplayStatus, string> = {
  draft: "border-line bg-surface text-fg-soft",
  open: "border-accent/30 bg-accent-soft text-accent",
  overdue: "border-danger/30 bg-danger-soft text-danger",
  paid: "border-success/30 bg-success-soft text-success",
  void: "border-line bg-surface text-fg-faint line-through",
};

export function StatusPill({
  status,
  legacy = false,
}: {
  status: DisplayStatus;
  legacy?: boolean;
}) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span
        className={`rounded-full border px-2 py-0.5 font-medium text-[10.5px] ${TONE[status]}`}
      >
        {STATUS_LABEL[status]}
      </span>
      {legacy && (
        <span
          className="rounded-full border border-line px-2 py-0.5 text-[10px] text-fg-faint"
          title="Recorded before invoice documents: no number or PDF"
        >
          Legacy
        </span>
      )}
    </span>
  );
}
