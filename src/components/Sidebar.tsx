import type { ReactNode } from "react";

export type SidebarView =
  | "home"
  | "sessions"
  | "focus"
  | "meetings"
  | "breaks"
  | "categorization"
  | "distraction-blocker"
  | "reports"
  | "ai-agent"
  | "integrations"
  | "trackers"
  | "settings";

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
      title={label}
      onClick={onSelect}
      aria-current={active ? "page" : undefined}
      className={`flex w-full items-center gap-2.5 rounded-[9px] px-2.5 py-2.5 text-left text-[13.5px] text-fg-soft transition-colors ${
        active ? "bg-accent-soft" : "hover:bg-surface"
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
      <span className="min-w-0 truncate">{label}</span>
    </button>
  );
}

interface NavItem {
  view: SidebarView;
  label: string;
  icon: ReactNode;
}

const coreNav: NavItem[] = [
  {
    view: "home",
    label: "Home",
    icon: (
      <>
        <path d="M4 11.5 12 4l8 7.5" />
        <path d="M6 10v9h12v-9" />
      </>
    ),
  },
  {
    view: "sessions",
    label: "Sessions",
    icon: (
      <>
        <rect x="3.5" y="5" width="17" height="15" rx="2" />
        <path d="M3.5 9.5h17M8 3v4M16 3v4" />
      </>
    ),
  },
  {
    view: "focus",
    label: "Focus",
    icon: (
      <>
        <circle cx="12" cy="12" r="7.5" />
        <circle cx="12" cy="12" r="3" />
      </>
    ),
  },
  {
    view: "meetings",
    label: "Meetings",
    icon: (
      <>
        <rect x="3.5" y="6" width="13" height="12" rx="2" />
        <path d="M16.5 10.5 20.5 8v8l-4-2.5" />
      </>
    ),
  },
  {
    view: "breaks",
    label: "Breaks",
    icon: (
      <>
        <path d="M5 9h11v6a4 4 0 0 1-4 4H9a4 4 0 0 1-4-4V9Z" />
        <path d="M16 10.5h1.5a2.5 2.5 0 0 1 0 5H16" />
        <path d="M8 5.5c0-1 .8-1 .8-2M12 5.5c0-1 .8-1 .8-2" />
      </>
    ),
  },
  {
    view: "categorization",
    label: "Clients & Projects",
    icon: (
      <>
        <rect x="3.5" y="8" width="17" height="11" rx="2" />
        <path d="M8.5 8V6a2 2 0 0 1 2-2h3a2 2 0 0 1 2 2v2M3.5 13h17" />
      </>
    ),
  },
  {
    view: "distraction-blocker",
    label: "Distraction Blocker",
    icon: (
      <>
        <path d="M12 3.5 19 6.5v5.5c0 4.5-3 7-7 8.5-4-1.5-7-4-7-8.5V6.5Z" />
        <path d="M9.5 12l1.8 1.8L14.7 10" />
      </>
    ),
  },
  {
    view: "reports",
    label: "Dashboards & Reports",
    icon: <path d="M4 20V10M10 20V4M16 20v-7M4 20h16" />,
  },
  {
    view: "ai-agent",
    label: "AI Agent",
    icon: (
      <>
        <path d="M12 3l1.6 4.4L18 9l-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6L12 3Z" />
        <path d="M19 15l.7 1.9L21.5 17.5l-1.9.7L19 20l-.7-1.8-1.8-.7 1.8-.6L19 15Z" />
      </>
    ),
  },
  {
    view: "integrations",
    label: "Integrations",
    icon: (
      <>
        <path d="M9 15l6-6" />
        <path d="M8 10 6 8a3.5 3.5 0 0 1 5-5l2 2M16 14l2 2a3.5 3.5 0 0 1-5 5l-2-2" />
      </>
    ),
  },
];

export function Sidebar({ view, onSelect }: SidebarProps) {
  return (
    <aside className="flex min-h-0 flex-col border-r border-line bg-rail px-3.5 py-4.5">
      {/* The only scrolling region. Its own box, so a long nav never moves the
          main page, and the page never moves the nav. */}
      <nav className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto">
        {coreNav.map((item) => (
          <NavTab
            key={item.view}
            active={view === item.view}
            label={item.label}
            onSelect={() => onSelect(item.view)}
          >
            {item.icon}
          </NavTab>
        ))}

        {/* Manual Trackers is a separate feature from automatic Sessions
            tracking above it, not a step in that pipeline — the divider
            marks that split rather than implying an order. */}
        <div className="my-1.5 border-t border-line" />

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
