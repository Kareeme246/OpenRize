export type Status = "not-implemented" | "in-progress";

const styles: Record<Status, string> = {
  "not-implemented": "border-white/10 bg-white/5 text-white/40",
  "in-progress": "border-accent/30 bg-accent-soft text-accent",
};

const labels: Record<Status, string> = {
  "not-implemented": "Not implemented",
  "in-progress": "In progress",
};

interface StatusBadgeProps {
  status: Status;
  className?: string;
}

export function StatusBadge({ status, className = "" }: StatusBadgeProps) {
  return (
    <span
      className={`shrink-0 rounded-full border px-2 py-0.5 font-mono text-[9.5px] uppercase tracking-wider ${styles[status]} ${className}`}
    >
      {labels[status]}
    </span>
  );
}
