import { type ReactNode, useId, useState } from "react";

export function EditorSection({
  title,
  summary,
  defaultOpen = false,
  children,
}: {
  title: string;
  summary?: ReactNode;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  const bodyId = useId();
  return (
    <section className="shape-bleed-table rounded-xl border border-line bg-panel">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={bodyId}
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-3 px-4 py-3 text-left"
      >
        <span className="font-semibold text-[13px] text-fg-strong">
          {title}
        </span>
        {!open && summary && (
          <span className="min-w-0 flex-1 truncate text-[11.5px] text-fg-soft">
            {summary}
          </span>
        )}
        <span
          aria-hidden="true"
          className={`ml-auto text-fg-faint transition-transform ${open ? "rotate-180" : ""}`}
        >
          ⌄
        </span>
      </button>
      {open && (
        <div id={bodyId} className="space-y-3 px-4 pb-4 text-[12px]">
          {children}
        </div>
      )}
    </section>
  );
}
