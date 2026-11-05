import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { useDismiss } from "../pages/timesheets/useDismiss";

/** One row of a `KebabMenu`. */
export type MenuEntry =
  | "divider"
  | {
      text: string;
      onSelect: () => void;
      danger?: boolean;
      disabled?: boolean;
      /** Shown on hover while disabled. */
      hint?: string;
      /** Marks the current choice in a list of choices. */
      checked?: boolean;
    }
  /** Opens its entries in place of the menu, with a way back. */
  | { text: string; entries: MenuEntry[] };

interface KebabMenuProps {
  /** The button's accessible name, such as "Options for Email". */
  label: string;
  entries: MenuEntry[];
  /** Positions the menu's anchor; it must establish a containing block. */
  className: string;
}

/** A three-dot button and its menu, as a stopwatch card and a section use. */
export function KebabMenu({ label, entries, className }: KebabMenuProps) {
  const [open, setOpen] = useState(false);
  /** The entry whose list is showing in place of the menu, if any. */
  const [view, setView] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => {
    setOpen(false);
    setView(null);
  }, []);
  useDismiss(open, ref, close);

  // Near the bottom of the window the menu opens upward instead of past the
  // edge, measured before paint so it never flickers.
  const menuRef = useRef<HTMLDivElement>(null);
  const [above, setAbove] = useState(false);
  useLayoutEffect(() => {
    void view;
    if (!open) {
      setAbove(false);
      return;
    }
    const menu = menuRef.current;
    if (menu === null) return;
    const { top, bottom } = menu.getBoundingClientRect();
    const height = bottom - top;
    const button = menu.parentElement?.getBoundingClientRect();
    setAbove(
      bottom > window.innerHeight - 8 &&
        button !== undefined &&
        button.top - height > 8,
    );
  }, [open, view]);

  const sub = entries.find(
    (entry): entry is { text: string; entries: MenuEntry[] } =>
      entry !== "divider" && "entries" in entry && entry.text === view,
  );
  const shown = sub?.entries ?? entries;

  const row = (entry: MenuEntry, index: number) => {
    if (entry === "divider") {
      return <hr key={`divider-${index}`} className="my-1 border-line" />;
    }
    const opensView = "entries" in entry;
    const danger = !opensView && entry.danger === true;
    const disabled = !opensView && entry.disabled === true;
    // A choice in a list of choices is a radio item that says if it is on.
    const role =
      !opensView && "checked" in entry
        ? ({
            role: "menuitemradio",
            "aria-checked": entry.checked === true,
          } as const)
        : ({ role: "menuitem" } as const);
    return (
      <button
        key={entry.text}
        type="button"
        {...role}
        aria-haspopup={opensView ? "menu" : undefined}
        disabled={disabled}
        title={!opensView && disabled ? entry.hint : undefined}
        onClick={() => {
          if (opensView) {
            setView(entry.text);
            return;
          }
          close();
          entry.onSelect();
        }}
        className={`flex w-full items-center justify-between gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] disabled:cursor-not-allowed disabled:opacity-40 ${
          danger
            ? "text-danger hover:bg-danger-soft"
            : "text-fg-muted hover:bg-surface hover:text-fg disabled:hover:bg-transparent"
        }`}
      >
        <span className="min-w-0 truncate">{entry.text}</span>
        {opensView ? (
          <span aria-hidden="true" className="text-fg-faint">
            ›
          </span>
        ) : (
          entry.checked === true && (
            <span aria-hidden="true" className="text-accent">
              ✓
            </span>
          )
        )}
      </button>
    );
  };

  return (
    <div ref={ref} className={className}>
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        onClick={() => (open ? close() : setOpen(true))}
        className={`grid size-7 place-items-center rounded-md transition-colors hover:bg-surface hover:text-fg ${
          open ? "bg-surface text-fg" : "text-fg-faint"
        }`}
      >
        <svg
          viewBox="0 0 24 24"
          className="size-4"
          fill="currentColor"
          aria-hidden="true"
        >
          <circle cx="12" cy="5" r="1.7" />
          <circle cx="12" cy="12" r="1.7" />
          <circle cx="12" cy="19" r="1.7" />
        </svg>
      </button>
      {open && (
        <div
          ref={menuRef}
          role="menu"
          aria-label={sub?.text}
          className={`absolute right-0 z-30 ${above ? "bottom-full mb-1" : "top-full mt-1"} w-48 rounded-lg border border-line bg-panel p-1 shadow-black/40 shadow-xl`}
        >
          {sub !== undefined && (
            <>
              <button
                type="button"
                onClick={() => setView(null)}
                className="flex w-full items-center gap-1.5 rounded-md px-2.5 py-1.5 text-left font-semibold text-[12px] text-fg-soft hover:bg-surface hover:text-fg"
              >
                <span aria-hidden="true" className="text-fg-faint">
                  ‹
                </span>
                {sub.text}
              </button>
              <hr className="my-1 border-line" />
            </>
          )}
          {shown.map(row)}
        </div>
      )}
    </div>
  );
}
