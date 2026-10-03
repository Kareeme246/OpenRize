import type { ReactNode } from "react";
import { useSettings } from "../hooks/useSettings";
import { PLATFORM } from "../lib/platform";
import { Tooltip } from "./Tooltip";

interface TopBarProps {
  canGoBack: boolean;
  canGoForward: boolean;
  onBack: () => void;
  onForward: () => void;
  sidebarCollapsed: boolean;
  onToggleSidebar: () => void;
  workflowPage?: boolean;
}

/**
 * The app's own bar. On macOS `titleBarStyle: "Overlay"` (tauri.conf.json)
 * draws the webview under a transparent title bar, so this row *is* the title
 * bar and must leave room for the traffic lights on the left. Elsewhere it is a
 * normal toolbar under the native title bar and needs no inset.
 */
export function TopBar({
  canGoBack,
  canGoForward,
  onBack,
  onForward,
  sidebarCollapsed,
  onToggleSidebar,
  workflowPage,
}: TopBarProps) {
  const { settings } = useSettings();
  return (
    <header
      data-tauri-drag-region="deep"
      className={`flex h-11 shrink-0 items-center gap-2 border-b border-line bg-bar pr-3 ${
        PLATFORM === "macos" ? "pl-[84px]" : "pl-3"
      }`}
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
        {workflowPage && settings.advancedWorkflowTracking && (
          <span className="select-none whitespace-nowrap text-[12px] font-semibold text-fg-muted">
            AI Workflow View
          </span>
        )}
      </div>

      <span className="shrink-0 select-none font-mono text-[11.5px] font-semibold tracking-wide text-fg-soft">
        OpenRize
      </span>

      <div className="flex-1" />
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
