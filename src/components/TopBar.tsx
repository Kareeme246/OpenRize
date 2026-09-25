interface TopBarProps {
  canGoBack: boolean;
  canGoForward: boolean;
  onBack: () => void;
  onForward: () => void;
}

const isMac = navigator.userAgent.includes("Mac");

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
}: TopBarProps) {
  return (
    <header
      data-tauri-drag-region="deep"
      className={`flex h-11 shrink-0 items-center gap-2 border-b border-line bg-bar pr-3 ${
        isMac ? "pl-[84px]" : "pl-3"
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
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={!enabled}
      onClick={onClick}
      className="grid size-7 shrink-0 place-items-center rounded-lg text-fg-soft hover:bg-surface-strong disabled:pointer-events-none disabled:text-fg-ghost"
    >
      <svg
        viewBox="0 0 24 24"
        className="size-3.5"
        fill="none"
        stroke="currentColor"
        strokeWidth={2.2}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d={path} />
      </svg>
    </button>
  );
}
