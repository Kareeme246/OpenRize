import { type DisplayStatus, STATUS_LABEL } from "../../lib/invoices";

const TONE: Record<DisplayStatus, string> = {
  draft: "border-line bg-surface text-fg-soft",
  open: "border-accent/30 bg-accent-soft text-accent",
  overdue: "border-danger/30 bg-danger-soft text-danger",
  paid: "border-success/30 bg-success-soft text-success",
  void: "border-line bg-surface text-fg-faint line-through",
};

export function StatusPill({ status }: { status: DisplayStatus }) {
  return (
    <span
      className={`inline-block rounded-full border px-2 py-0.5 font-medium text-[10.5px] ${TONE[status]}`}
    >
      {STATUS_LABEL[status]}
    </span>
  );
}
