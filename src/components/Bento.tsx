import type { ReactNode } from "react";
import { type Status, StatusBadge } from "./StatusBadge";

/** Bento card sizes. Kept as literal class strings (not built from template
 * pieces) so Tailwind's source scanner can see every utility it needs to
 * generate. */
export type BentoSpan = "sm" | "md" | "lg" | "wide" | "tall";

const spanClasses: Record<BentoSpan, string> = {
  sm: "col-span-1 row-span-1",
  md: "col-span-2 row-span-1",
  lg: "col-span-2 row-span-2",
  wide: "col-span-4 row-span-1",
  tall: "col-span-1 row-span-2",
};

interface PageHeaderProps {
  title: string;
  description: string;
  status: Status;
}

export function PageHeader({ title, description, status }: PageHeaderProps) {
  return (
    <header className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <h1 className="text-[15px] font-semibold">{title}</h1>
        <div className="font-mono text-[10.5px] text-white/35">
          {description}
        </div>
      </div>
      <StatusBadge status={status} className="mt-0.5" />
    </header>
  );
}

export function BentoGrid({ children }: { children: ReactNode }) {
  return (
    <div className="grid grid-cols-4 auto-rows-[132px] gap-3">{children}</div>
  );
}

interface BentoCardProps {
  title: string;
  description: string;
  status?: Status;
  span?: BentoSpan;
  icon?: ReactNode;
  children?: ReactNode;
}

export function BentoCard({
  title,
  description,
  status = "not-implemented",
  span = "sm",
  icon,
  children,
}: BentoCardProps) {
  return (
    <div
      className={`flex min-w-0 flex-col gap-2 overflow-hidden rounded-xl border border-white/10 bg-linear-to-b from-white/5 to-white/1 p-4 ${spanClasses[span]}`}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="flex min-w-0 items-start gap-2 text-white/70">
          {icon}
          <h3 className="text-[13px] font-semibold leading-snug text-white">
            {title}
          </h3>
        </div>
        <StatusBadge status={status} className="mt-0.5" />
      </div>
      <p className="text-[11.5px] leading-snug text-white/40">{description}</p>
      {children}
    </div>
  );
}
