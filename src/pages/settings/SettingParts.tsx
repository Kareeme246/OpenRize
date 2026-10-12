import type { ReactNode } from "react";
import { Picker } from "../../components/Picker";
import { type Status, StatusBadge } from "../../components/StatusBadge";

/** Title + description with an inline control on the right. */
export function SettingRow({
  title,
  description,
  status,
  children,
}: {
  title: string;
  description: string;
  status?: Status;
  children?: ReactNode;
}) {
  return (
    <div className="setting-row flex items-center justify-between gap-4 border-b border-line px-4 py-3 last:border-b-0">
      <div className="min-w-0">
        <div className="text-[13px] font-medium text-fg">{title}</div>
        <div className="text-[12px] leading-relaxed text-fg-muted">
          {description}
        </div>
      </div>
      {children ?? (status !== undefined && <StatusBadge status={status} />)}
    </div>
  );
}

/** Title + description with a full-width control underneath. */
export function SettingBlock({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="setting-block border-b border-line px-4 py-3 last:border-b-0">
      <div className="text-[13px] font-medium text-fg">{title}</div>
      <div className="text-[12px] leading-relaxed text-fg-muted">
        {description}
      </div>
      <div className="mt-3">{children}</div>
    </div>
  );
}

export function SettingGroup({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2">
      <h2 className="px-1 font-mono text-[11px] font-semibold uppercase tracking-wider text-fg-muted">
        {title}
      </h2>
      <div className="settings-group-card shape-bleed-table rounded-xl border border-settings-card-border bg-settings-card">
        {children}
      </div>
    </section>
  );
}

/** A themed dropdown, styled to match the segmented controls beside it. */
export function Select({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: number;
  options: { value: number; label: string }[];
  onChange: (value: number) => void;
}) {
  return (
    <Picker<number>
      ariaLabel={label}
      value={value}
      options={options}
      onChange={onChange}
      variant="compact"
    />
  );
}
