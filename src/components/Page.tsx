import type { ReactNode } from "react";
import { Picker, type PickerOption } from "./Picker";
import { SegmentedControl, type SegmentedOption } from "./SegmentedControl";

/**
 * Shared view chrome (design board, D shared conventions): a header with a
 * title and breadcrumb, then ‹ Today ›, then a Day | Week | Month control;
 * lists with a written empty state, a skeleton while loading, and an inline
 * error with Retry.
 */

export function PageHeader({
  title,
  crumb,
  children,
}: {
  title: string;
  crumb?: string;
  children?: ReactNode;
}) {
  return (
    <header className="flex h-12 shrink-0 items-center justify-between gap-3 border-line border-b px-5">
      <h1 className="flex min-w-0 items-baseline gap-1.5 truncate font-semibold text-[15px] text-fg-strong">
        {title}
        {crumb && (
          <span className="truncate font-normal text-[13px] text-fg-soft">
            / {crumb}
          </span>
        )}
      </h1>
      <div className="flex shrink-0 items-center gap-2">{children}</div>
    </header>
  );
}

const STEP_BUTTON =
  "flex size-7 items-center justify-center rounded-md border border-line bg-panel text-fg-soft transition-colors hover:bg-surface hover:text-fg";

export function DateStepper({
  unit,
  onStep,
  onToday,
}: {
  /** "day", "week", or "month", for the button labels. */
  unit: string;
  onStep: (direction: 1 | -1) => void;
  onToday: () => void;
}) {
  return (
    <div className="flex items-center gap-1">
      <button
        type="button"
        onClick={() => onStep(-1)}
        className={STEP_BUTTON}
        title={`Previous ${unit}`}
        aria-label={`Previous ${unit}`}
      >
        ‹
      </button>
      <button
        type="button"
        onClick={onToday}
        className="h-7 rounded-md border border-line bg-panel px-2.5 font-medium text-[12px] text-fg-soft transition-colors hover:bg-surface hover:text-fg"
      >
        Today
      </button>
      <button
        type="button"
        onClick={() => onStep(1)}
        className={STEP_BUTTON}
        title={`Next ${unit}`}
        aria-label={`Next ${unit}`}
      >
        ›
      </button>
    </div>
  );
}

type Scale = "day" | "week" | "month";

const SCALE_OPTIONS: SegmentedOption<Scale>[] = [
  { value: "day", label: "Day" },
  { value: "week", label: "Week" },
  { value: "month", label: "Month" },
];

export function ScaleControl({
  name,
  value,
  onChange,
}: {
  name: string;
  value: Scale;
  onChange: (scale: Scale) => void;
}) {
  return (
    <SegmentedControl
      name={name}
      value={value}
      options={SCALE_OPTIONS}
      onChange={onChange}
    />
  );
}

export interface TabOption<T extends string> {
  value: T;
  label: string;
  count?: number;
}

/** Underlined tabs, the review panel's style, with optional counts. */
export function Tabs<T extends string>({
  tabs,
  value,
  onChange,
  label,
}: {
  tabs: TabOption<T>[];
  value: T;
  onChange: (value: T) => void;
  label: string;
}) {
  return (
    <div
      role="tablist"
      aria-label={label}
      className="flex gap-4 border-line border-b font-medium text-[12.5px]"
    >
      {tabs.map((tab) => {
        const active = tab.value === value;
        return (
          <button
            key={tab.value}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(tab.value)}
            className={`-mb-px flex items-center gap-1.5 border-b-2 pb-2 transition-colors ${
              active
                ? "border-accent font-semibold text-fg-strong"
                : "border-transparent text-fg-soft hover:text-fg"
            }`}
          >
            {tab.label}
            {tab.count !== undefined && (
              <span
                className={`rounded-full px-1.5 font-mono text-[10.5px] tabular-nums ${
                  active
                    ? "bg-accent-soft text-accent"
                    : "bg-surface text-fg-faint"
                }`}
              >
                {tab.count}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

export function StatCard({
  label,
  value,
  sub,
  active,
  tone,
  onClick,
  children,
}: {
  label: string;
  value: string;
  sub?: ReactNode;
  active?: boolean;
  tone?: "review" | "accent";
  onClick?: () => void;
  children?: ReactNode;
}) {
  const body = (
    <>
      <div className="font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
        {label}
      </div>
      <div
        className={`mt-1 font-semibold text-[20px] tabular-nums leading-tight ${
          tone === "review"
            ? "text-review"
            : tone === "accent"
              ? "text-accent"
              : "text-fg-strong"
        }`}
      >
        {value}
      </div>
      {sub && <div className="mt-0.5 text-[11.5px] text-fg-soft">{sub}</div>}
      {children}
    </>
  );
  // Top-aligned so values line up across cards with and without a sub line.
  const frame = `flex min-w-0 flex-col justify-start rounded-xl border bg-panel p-3 text-left ${
    active ? "border-accent/50 ring-1 ring-accent/30" : "border-line"
  }`;
  if (!onClick) return <div className={frame}>{body}</div>;
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={`${frame} transition-colors hover:border-fg-soft/40`}
    >
      {body}
    </button>
  );
}

/** A thin progress bar; amber once `warn` is reached, and when over 100%. */
export function Progress({
  value,
  warnAt,
  label,
}: {
  /** 0–1, may exceed 1. */
  value: number;
  warnAt?: number;
  label: string;
}) {
  const warn = warnAt !== undefined && value >= warnAt;
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(Math.min(1, value) * 100)}
      className="h-1.5 w-full overflow-hidden rounded-full bg-surface-strong"
    >
      <div
        className={`h-full rounded-full transition-all ${warn ? "bg-review" : "bg-accent"}`}
        style={{ width: `${Math.min(100, Math.max(0, value * 100))}%` }}
      />
    </div>
  );
}

export function EmptyState({
  title,
  hint,
  action,
}: {
  title: string;
  hint?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-center justify-center gap-1 px-6 py-12 text-center">
      <p className="font-medium text-[13px] text-fg-muted">{title}</p>
      {hint && <p className="max-w-sm text-[11.5px] text-fg-faint">{hint}</p>}
      {action && <div className="mt-3">{action}</div>}
    </div>
  );
}

export function InlineError({
  message,
  onRetry,
}: {
  message: string;
  onRetry?: () => void;
}) {
  return (
    <div
      role="alert"
      className="flex items-center gap-3 rounded-lg border border-danger/30 bg-danger-soft px-3 py-2 text-[12px]"
    >
      <span className="min-w-0 flex-1 text-danger">{message}</span>
      {onRetry && (
        <button
          type="button"
          onClick={onRetry}
          className="shrink-0 rounded-md border border-danger/40 px-2 py-0.5 font-semibold text-danger hover:bg-danger/10"
        >
          Retry
        </button>
      )}
    </div>
  );
}

/** Placeholder rows while a list loads. */
export function SkeletonRows({ rows = 5 }: { rows?: number }) {
  return (
    <div className="space-y-2 p-3" aria-hidden="true">
      {Array.from({ length: rows }, (_, index) => index).map((index) => (
        <div
          key={`skeleton-${index}`}
          className="entry-processing h-8 rounded-md"
          style={{ opacity: 1 - index * 0.12 }}
        />
      ))}
    </div>
  );
}

export const BUTTON_PRIMARY =
  "rounded-md bg-accent px-3 py-1.5 font-semibold text-[12px] text-accent-fg transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40";
export const BUTTON_SECONDARY =
  "rounded-md border border-line bg-panel px-3 py-1.5 font-medium text-[12px] text-fg-soft transition-colors hover:bg-surface hover:text-fg disabled:cursor-not-allowed disabled:opacity-40";
const FIELD_BASE =
  "w-full rounded-md border border-line bg-surface px-2.5 py-1.5 text-[12px] text-fg outline-hidden focus:border-accent";
/** A sheet's input or select; the fixed height keeps the two kinds level. */
export const FIELD = `${FIELD_BASE} h-8`;
export const TEXTAREA_FIELD = FIELD_BASE;

/** A themed picker dressed as a filter pill ("Category ▾"). */
export function FilterSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: { value: string; label: string; color?: string }[];
  onChange: (value: string) => void;
}) {
  const allOptions: PickerOption<string>[] = options.some((o) => o.value === "")
    ? options
    : [{ value: "", label }, ...options];

  return (
    <Picker<string>
      ariaLabel={label}
      value={value}
      options={allOptions}
      onChange={onChange}
      variant="filter"
      placeholder={label}
    />
  );
}

/** The colour dot every category and project name carries. */
export function Dot({ color, size = 8 }: { color: string; size?: number }) {
  return (
    <span
      aria-hidden="true"
      className="inline-block shrink-0 rounded-full"
      style={{ backgroundColor: color, width: size, height: size }}
    />
  );
}
