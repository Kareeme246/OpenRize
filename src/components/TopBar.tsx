import type { ReactNode } from "react";
import { HAS_NATIVE_CAPTION, PLATFORM } from "../lib/platform";
import type { Route } from "../lib/types";
import { AccountSwitcher } from "./AccountSwitcher";
import { Tooltip } from "./Tooltip";

interface TopBarProps {
  canGoBack: boolean;
  canGoForward: boolean;
  onBack: () => void;
  onForward: () => void;
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  onNavigate: (route: Route) => void;
}

/**
 * The app's own bar. On macOS `titleBarStyle: "Overlay"` (tauri.conf.json)
 * draws the webview under a transparent title bar, so this row *is* the title
 * bar and must leave room for the traffic lights on the left, and it names the
 * app. Elsewhere it is a slimmer toolbar under the native caption, which
 * already names the app and carries the window buttons.
 */
export function TopBar({
  canGoBack,
  canGoForward,
  onBack,
  onForward,
  sidebarCollapsed,
  onToggleSidebar,
  onNavigate,
}: TopBarProps) {
  return (
    <header
      data-tauri-drag-region="deep"
      className={`flex shrink-0 items-center gap-2 border-b border-line bg-bar pr-3 ${
        HAS_NATIVE_CAPTION ? "h-10" : "h-11"
      } ${PLATFORM === "macos" ? "pl-[84px]" : "pl-3"}`}
    >
      <div className="flex flex-1 items-center gap-2">
        <NavArrow
          label="Back"
          enabled={canGoBack}
          onClick={onBack}
          path="M15 5l-7 7 7 7"
        />
        <NavArrow
          label="Forward"
          enabled={canGoForward}
          onClick={onForward}
          path="M9 5l7 7-7 7"
        />
        <IconButton
          label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
          onClick={onToggleSidebar}
          large
        >
          <rect x="3" y="4" width="18" height="16" rx="2" />
          <path d="M9 4v16" />
          {sidebarCollapsed ? (
            <path d="M14 10l2 2-2 2" />
          ) : (
            <path d="M16 10l-2 2 2 2" />
          )}
        </IconButton>
        <AccountSwitcher onNavigate={onNavigate} />
      </div>

      {!HAS_NATIVE_CAPTION && (
        <>
          <span className="shrink-0 select-none font-mono text-[11.5px] font-semibold tracking-wide text-fg-soft">
            OpenRize
          </span>

          <div className="flex-1" />
        </>
      )}
    </header>
  );
}

function NavArrow({
  label,
  path,
  enabled,
  onClick,
}: {
  label: string;
  path: string;
  enabled: boolean;
  onClick: () => void;
}) {
  return (
    <IconButton label={label} disabled={!enabled} onClick={onClick}>
      <path d={path} />
    </IconButton>
  );
}

function IconButton({
  label,
  disabled = false,
  large = false,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  large?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <Tooltip content={label}>
      <button
        type="button"
        aria-label={label}
        disabled={disabled}
        onClick={onClick}
        className="grid size-7 shrink-0 place-items-center rounded-lg text-fg-soft hover:bg-surface-strong disabled:pointer-events-none disabled:text-fg-ghost"
      >
        <svg
          viewBox="0 0 24 24"
          className={large ? "size-[18px]" : "size-3.5"}
          fill="none"
          stroke="currentColor"
          strokeWidth={2.2}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          {children}
        </svg>
      </button>
    </Tooltip>
  );
}
