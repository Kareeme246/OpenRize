import { type ReactNode, useRef } from "react";
import {
  DRAWS_CAPTION_BUTTONS,
  HAS_NATIVE_CAPTION,
  TOP_BAR_PADDING_LEFT,
  TOP_BAR_PADDING_RIGHT,
} from "../lib/platform";
import type { Route } from "../lib/types";
import { AccountSwitcher } from "./AccountSwitcher";
import { CaptionButtons, useWindowMenu } from "./CaptionButtons";
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
 * The app's own bar: the title bar on macOS (`titleBarStyle: "Overlay"`, leave
 * room for the traffic lights) and Windows (native caption removed,
 * caption.rs); elsewhere a toolbar under the caption.
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
  const bar = useRef<HTMLElement>(null);
  useWindowMenu(bar);
  return (
    <header
      data-tauri-drag-region={HAS_NATIVE_CAPTION ? undefined : "deep"}
      ref={bar}
      className={`flex shrink-0 items-center gap-2 border-b border-line bg-bar ${
        HAS_NATIVE_CAPTION ? "h-10" : "h-11"
      } ${TOP_BAR_PADDING_LEFT} ${TOP_BAR_PADDING_RIGHT}`}
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

      {DRAWS_CAPTION_BUTTONS && <CaptionButtons />}
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
