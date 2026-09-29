import { type ReactNode, useCallback, useRef, useState } from "react";
import { Dot } from "../../components/Page";
import type { Catalog } from "../../hooks/useCatalog";
import { hasTimesheetFilters, NONE } from "../../lib/timesheetGrid";
import type { TimesheetFilters } from "../../lib/types";
import { useDismiss } from "./useDismiss";

type Dimension = keyof TimesheetFilters;

interface Option {
  value: string;
  label: string;
  color?: string;
}

interface Section {
  key: Dimension;
  label: string;
  icon: ReactNode;
  options: Option[];
}

/** A 24px stroke glyph at the chip's size. */
function Glyph({ children }: { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className="size-3.5 shrink-0"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

const ICONS: Record<Dimension, ReactNode> = {
  clientIds: (
    <Glyph>
      <rect x="4" y="3" width="16" height="18" rx="2" />
      <circle cx="12" cy="10" r="2.5" />
      <path d="M8 17c.8-1.8 2.2-2.7 4-2.7s3.2.9 4 2.7" />
    </Glyph>
  ),
  projectIds: (
    <Glyph>
      <path d="M12 3 20 7.5v9L12 21l-8-4.5v-9z" />
      <path d="m4 7.5 8 4.5 8-4.5M12 12v9" />
    </Glyph>
  ),
  categoryIds: (
    <Glyph>
      <path d="M3 12V4a1 1 0 0 1 1-1h8l9 9-9 9z" />
      <circle cx="8" cy="8" r="1.4" />
    </Glyph>
  ),
};

function Checklist({
  section,
  selected,
  onToggle,
}: {
  section: Section;
  selected: string[];
  onToggle: (value: string) => void;
}) {
  return (
    <fieldset className="min-w-0">
      <legend className="flex w-full items-center gap-1.5 px-2 pt-1 pb-1.5 font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
        {section.icon}
        {section.label}
      </legend>
      <div className="max-h-56 overflow-y-auto overscroll-contain">
        {section.options.map((option) => (
          <label
            key={option.value}
            className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-[12px] text-fg-muted hover:bg-surface hover:text-fg"
          >
            <input
              type="checkbox"
              checked={selected.includes(option.value)}
              onChange={() => onToggle(option.value)}
              className="accent-(--accent)"
            />
            {option.color && <Dot color={option.color} />}
            <span className="truncate">{option.label}</span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}

/**
 * Rise's filter row: a `Filters` button that opens every dimension at once,
 * then one chip per dimension (Clients, Projects, Categories) that opens
 * just its own list. Both edit the same selection.
 */
export function FilterBar({
  catalog,
  filters,
  onChange,
}: {
  catalog: Catalog;
  filters: TimesheetFilters;
  onChange: (filters: TimesheetFilters) => void;
}) {
  const [open, setOpen] = useState<Dimension | "all" | null>(null);
  /** A chip's list opens under that chip. */
  const [anchor, setAnchor] = useState(0);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(null), []);
  useDismiss(open !== null, ref, close);

  const byName = (a: Option, b: Option): number =>
    a.label.localeCompare(b.label);
  const sections: Section[] = [
    {
      key: "clientIds",
      label: "Clients",
      icon: ICONS.clientIds,
      options: [
        ...catalog.clients
          .map((client) => ({ value: client.id, label: client.name }))
          .sort(byName),
        { value: NONE, label: "No client" },
      ],
    },
    {
      key: "projectIds",
      label: "Projects",
      icon: ICONS.projectIds,
      options: [
        ...catalog.projects
          .map((project) => ({
            value: project.id,
            label: project.name,
            color: project.color,
          }))
          .sort(byName),
        { value: NONE, label: "No project", color: "var(--fg-ghost)" },
      ],
    },
    {
      key: "categoryIds",
      label: "Categories",
      icon: ICONS.categoryIds,
      options: [
        ...catalog.categories
          .filter((category) => !category.archived)
          .map((category) => ({
            value: category.id,
            label: category.name,
            color: category.color,
          })),
        { value: NONE, label: "Uncategorized", color: "var(--fg-ghost)" },
      ],
    },
  ];

  const toggle = (key: Dimension, value: string): void => {
    const current = filters[key] ?? [];
    const next = current.includes(value)
      ? current.filter((id) => id !== value)
      : [...current, value];
    onChange({ ...filters, [key]: next.length ? next : undefined });
  };
  const active = hasTimesheetFilters(filters);
  const activeCount = sections.reduce(
    (sum, section) => sum + (filters[section.key]?.length ?? 0),
    0,
  );
  const shown =
    open === "all"
      ? sections
      : sections.filter((section) => section.key === open);

  return (
    <div ref={ref} className="relative flex items-center gap-2">
      <button
        type="button"
        onClick={() => setOpen(open === "all" ? null : "all")}
        aria-expanded={open === "all"}
        className={`flex h-8 items-center gap-1.5 rounded-md px-1.5 font-medium text-[12.5px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent ${
          open === "all" || active
            ? "text-fg-strong"
            : "text-fg-soft hover:text-fg"
        }`}
      >
        <Glyph>
          <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0" />
          <circle cx="16" cy="6" r="2" />
          <circle cx="10" cy="12" r="2" />
          <circle cx="18" cy="18" r="2" />
        </Glyph>
        Filters
        {activeCount > 0 && (
          <span className="rounded-full bg-accent-soft px-1.5 font-mono text-[10.5px] text-accent tabular-nums">
            {activeCount}
          </span>
        )}
      </button>
      <div className="flex overflow-hidden rounded-lg border border-line bg-panel">
        {sections.map((section, index) => {
          const count = filters[section.key]?.length ?? 0;
          const isOpen = open === section.key;
          return (
            <button
              key={section.key}
              type="button"
              onClick={(event) => {
                setAnchor(event.currentTarget.offsetLeft);
                setOpen(isOpen ? null : section.key);
              }}
              aria-expanded={isOpen}
              className={`flex h-8 items-center gap-1.5 px-3 font-medium text-[12.5px] outline-none transition-colors focus-visible:ring-2 focus-visible:ring-accent focus-visible:ring-inset ${
                index > 0 ? "border-line border-l" : ""
              } ${
                count > 0 || isOpen
                  ? "bg-accent-soft text-fg-strong"
                  : "text-fg-muted hover:bg-surface hover:text-fg"
              }`}
            >
              {section.icon}
              {section.label}
              {count > 0 && (
                <span className="font-mono text-[10.5px] text-accent tabular-nums">
                  {count}
                </span>
              )}
            </button>
          );
        })}
      </div>
      {active && (
        <button
          type="button"
          onClick={() => onChange({})}
          className="h-8 px-1.5 font-medium text-[12px] text-fg-soft hover:text-fg"
        >
          Clear
        </button>
      )}
      {open !== null && (
        <div
          className={`absolute top-full left-0 z-30 mt-1.5 grid gap-2 rounded-lg border border-line bg-panel p-1.5 shadow-black/40 shadow-xl ${
            open === "all" ? "w-[40rem] grid-cols-3" : "w-60"
          }`}
          style={open !== "all" ? { left: `${anchor}px` } : undefined}
        >
          {shown.map((section) => (
            <Checklist
              key={section.key}
              section={section}
              selected={filters[section.key] ?? []}
              onToggle={(value) => toggle(section.key, value)}
            />
          ))}
        </div>
      )}
    </div>
  );
}
