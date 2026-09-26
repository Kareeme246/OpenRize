import type { ReactNode } from "react";
import type { Route } from "../lib/types";

interface SidebarProps {
  currentRoute: Route;
  onNavigate: (route: Route) => void;
  pendingCount?: number;
  currentApp?: string;
  captureEnabled?: boolean;
  trackingActive?: boolean;
  onToggleCapture?: () => void;
}

interface NavTabProps {
  active: boolean;
  label: string;
  badge?: number;
  onSelect: () => void;
  children: ReactNode;
}

function NavTab({ active, label, badge, onSelect, children }: NavTabProps) {
  return (
    <button
      type="button"
      title={label}
      onClick={onSelect}
      aria-current={active ? "page" : undefined}
      className={`flex w-full items-center gap-2.5 rounded-[9px] px-2.5 py-2 text-left text-[13px] font-medium transition-colors ${
        active
          ? "bg-accent/15 text-accent font-semibold"
          : "text-fg-soft hover:bg-surface hover:text-fg"
      }`}
    >
      <svg
        viewBox="0 0 24 24"
        className="size-4 shrink-0"
        fill="none"
        stroke="currentColor"
        strokeWidth={1.8}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {children}
      </svg>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {badge !== undefined && badge > 0 && (
        <span className="ml-auto rounded-full bg-review/20 px-2 py-0.5 text-[11px] font-bold text-review">
          {badge}
        </span>
      )}
    </button>
  );
}

export function Sidebar({
  currentRoute,
  onNavigate,
  pendingCount = 0,
  currentApp,
  captureEnabled = true,
  trackingActive = captureEnabled,
  onToggleCapture,
}: SidebarProps) {
  const currentName = currentRoute.name;

  return (
    <aside className="flex h-full min-h-0 flex-col border-r border-line bg-rail px-3 py-3 select-none">
      <nav className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto pr-1">
        {/* Track Group */}
        <div className="flex flex-col gap-0.5">
          <span className="px-2 pb-1 text-[10.5px] font-semibold tracking-wider text-fg-faint uppercase">
            Track
          </span>
          <NavTab
            active={currentName === "calendar"}
            label="Calendar"
            badge={pendingCount}
            onSelect={() => onNavigate({ name: "calendar" })}
          >
            <rect x="3" y="4" width="18" height="18" rx="2" />
            <path d="M16 2v4M8 2v4M3 10h18" />
          </NavTab>
          <NavTab
            active={currentName === "timesheet"}
            label="My Timesheet"
            onSelect={() => onNavigate({ name: "timesheet" })}
          >
            <path d="M9 5H7a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V7a2 2 0 0 0-2-2h-2" />
            <rect x="9" y="3" width="6" height="4" rx="1" />
            <path d="M9 14l2 2 4-4" />
          </NavTab>
          <NavTab
            active={currentName === "apps"}
            label="Apps"
            onSelect={() => onNavigate({ name: "apps" })}
          >
            <rect x="3" y="3" width="7" height="7" rx="1.5" />
            <rect x="14" y="3" width="7" height="7" rx="1.5" />
            <rect x="14" y="14" width="7" height="7" rx="1.5" />
            <rect x="3" y="14" width="7" height="7" rx="1.5" />
          </NavTab>
        </div>

        {/* Analyze Group */}
        <div className="flex flex-col gap-0.5">
          <span className="px-2 pb-1 text-[10.5px] font-semibold tracking-wider text-fg-faint uppercase">
            Analyze
          </span>
          <NavTab
            active={currentName === "entries"}
            label="Time Entries"
            onSelect={() => onNavigate({ name: "entries" })}
          >
            <circle cx="12" cy="12" r="10" />
            <polyline points="12 6 12 12 16 14" />
          </NavTab>
          <NavTab
            active={currentName === "timesheets"}
            label="Timesheets"
            onSelect={() => onNavigate({ name: "timesheets" })}
          >
            <rect x="3" y="3" width="18" height="18" rx="2" />
            <path d="M3 9h18M3 15h18M9 3v18M15 3v18" />
          </NavTab>
        </div>

        {/* Work Group */}
        <div className="flex flex-col gap-0.5">
          <span className="px-2 pb-1 text-[10.5px] font-semibold tracking-wider text-fg-faint uppercase">
            Work
          </span>
          <NavTab
            active={currentName === "projects"}
            label="Projects"
            onSelect={() => onNavigate({ name: "projects" })}
          >
            <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
          </NavTab>
          <NavTab
            active={currentName === "invoices"}
            label="Invoices"
            onSelect={() => onNavigate({ name: "invoices" })}
          >
            <rect x="4" y="2" width="16" height="20" rx="2" />
            <line x1="8" y1="6" x2="16" y2="6" />
            <line x1="8" y1="10" x2="16" y2="10" />
            <line x1="8" y1="14" x2="12" y2="14" />
          </NavTab>
        </div>

        {/* Manual Group */}
        <div className="flex flex-col gap-0.5">
          <span className="px-2 pb-1 text-[10.5px] font-semibold tracking-wider text-fg-faint uppercase">
            Manual
          </span>
          <NavTab
            active={currentName === "timers"}
            label="Timers"
            onSelect={() => onNavigate({ name: "timers" })}
          >
            <circle cx="12" cy="13" r="8" />
            <path d="M12 9v4.2l3 1.8M9 2h6" />
          </NavTab>
        </div>
      </nav>

      {/* Bottom Pinned Controls */}
      <div className="mt-auto flex shrink-0 flex-col gap-2 pt-2 border-t border-line">
        <NavTab
          active={currentName === "settings"}
          label="Settings"
          onSelect={() => onNavigate({ name: "settings" })}
        >
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </NavTab>

        {/* Live capture status strip */}
        <div className="flex items-center gap-2 rounded-lg bg-surface px-2.5 py-1.5 text-[11.5px]">
          <span
            className={`size-2 shrink-0 rounded-full ${
              trackingActive ? "bg-accent animate-pulse" : "bg-review"
            }`}
          />
          <span className="min-w-0 flex-1 truncate text-fg-muted font-medium">
            {trackingActive
              ? currentApp
                ? `Tracking · ${currentApp}`
                : "Tracking active"
              : !captureEnabled
                ? "Tracking paused"
                : "Outside tracking hours"}
          </span>
          {onToggleCapture && (
            <button
              type="button"
              onClick={onToggleCapture}
              title={
                trackingActive
                  ? "Pause tracking"
                  : captureEnabled
                    ? "Start tracking"
                    : "Resume tracking"
              }
              className="rounded p-1 text-fg-soft hover:bg-surface-strong hover:text-fg transition-colors"
            >
              {trackingActive ? (
                <svg
                  viewBox="0 0 24 24"
                  className="size-3.5 fill-current"
                  aria-label="Pause"
                >
                  <rect x="6" y="4" width="4" height="16" rx="1" />
                  <rect x="14" y="4" width="4" height="16" rx="1" />
                </svg>
              ) : (
                <svg
                  viewBox="0 0 24 24"
                  className="size-3.5 fill-current"
                  aria-label="Resume"
                >
                  <polygon points="5 3 19 12 5 21 5 3" />
                </svg>
              )}
            </button>
          )}
        </div>
      </div>
    </aside>
  );
}
