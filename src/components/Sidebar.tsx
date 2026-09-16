import type { ReactNode } from "react";

export type SidebarView = "trackers" | "settings";

interface SidebarProps {
  view: SidebarView;
  onSelect: (view: SidebarView) => void;
}

interface NavTabProps {
  active: boolean;
  label: string;
  onSelect: () => void;
  children: ReactNode;
}

function NavTab({ active, label, onSelect, children }: NavTabProps) {
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={active ? "page" : undefined}
      className={`flex w-full items-center gap-2.5 rounded-[9px] px-2.5 py-2.5 text-left text-[13.5px] text-white/55 transition-colors ${
        active ? "bg-accent-soft" : "hover:bg-white/5"
      }`}
    >
      <svg
        viewBox="0 0 24 24"
        className="size-3.5 shrink-0"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {children}
      </svg>
      {label}
    </button>
  );
}

export function Sidebar({ view, onSelect }: SidebarProps) {
  return (
    <aside className="flex min-h-0 flex-col border-r border-white/10 bg-black/35 px-3.5 py-4.5">
      {/* The only scrolling region. Its own box, so a long nav never moves the
          main page, and the page never moves the nav. */}
      <nav className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto">
        <NavTab
          active={view === "trackers"}
          label="Trackers"
          onSelect={() => onSelect("trackers")}
        >
          <circle cx="12" cy="13" r="8" />
          <path d="M12 9v4.2l3 1.8M9 2h6" />
        </NavTab>
      </nav>

      <div className="mt-auto flex shrink-0 flex-col gap-1 pt-3">
        <NavTab
          active={view === "settings"}
          label="Settings"
          onSelect={() => onSelect("settings")}
        >
          <circle cx="12" cy="12" r="3.2" />
          <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-2.9 1.2 2 2 0 1 1-4 0 1.7 1.7 0 0 0-2.9-1.2l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1A1.7 1.7 0 0 0 3 15a2 2 0 1 1 0-4 1.7 1.7 0 0 0 1.2-2.9l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.7 1.7 0 0 0 9 4.6a2 2 0 1 1 4 0 1.7 1.7 0 0 0 2.9 1.2l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1A1.7 1.7 0 0 0 19.4 11a2 2 0 1 1 0 4Z" />
        </NavTab>
      </div>
    </aside>
  );
}
