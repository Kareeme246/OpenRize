import { useState } from "react";
import { useNotImplemented } from "./NotImplemented";

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
  const { show } = useNotImplemented();
  const [teamsOpen, setTeamsOpen] = useState(false);

  const teamItems = ["Create a team", "Join a team", "Switch workspace"];

  return (
    <header
      data-tauri-drag-region="deep"
      className={`flex h-11 shrink-0 items-center gap-2 border-b border-white/10 bg-black/30 pr-3 ${
        isMac ? "pl-[84px]" : "pl-3"
      }`}
    >
      <div className="relative flex flex-1 items-center gap-2">
        <button
          type="button"
          onClick={() => setTeamsOpen((open) => !open)}
          aria-haspopup="menu"
          aria-expanded={teamsOpen}
          className="flex items-center gap-1.5 rounded-lg border border-white/10 bg-white/5 px-2.5 py-1.5 text-[12.5px] font-medium text-white/75 hover:bg-white/10"
        >
          <svg
            viewBox="0 0 24 24"
            className="size-3.5"
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <circle cx="9" cy="8" r="3" />
            <path d="M3.5 19a5.5 5.5 0 0 1 11 0M16 6.2a3 3 0 0 1 0 5.6M17.5 19a5.5 5.5 0 0 0-2-4.2" />
          </svg>
          Teams
          <svg
            viewBox="0 0 24 24"
            className="size-3 text-white/45"
            fill="none"
            stroke="currentColor"
            strokeWidth={2.2}
            strokeLinecap="round"
            strokeLinejoin="round"
            aria-hidden="true"
          >
            <path d="m6 9 6 6 6-6" />
          </svg>
        </button>

        {teamsOpen && (
          <>
            {/* Transparent full-window button: closes on any outside click
                without a document listener. */}
            <button
              type="button"
              aria-label="Close teams menu"
              onClick={() => setTeamsOpen(false)}
              className="fixed inset-0 z-40 cursor-default"
            />
            <div
              role="menu"
              className="absolute left-0 top-full z-50 mt-1 w-48 overflow-hidden rounded-xl border border-white/10 bg-ink-900 py-1 shadow-2xl"
            >
              {teamItems.map((item) => (
                <button
                  key={item}
                  type="button"
                  role="menuitem"
                  onClick={() => {
                    setTeamsOpen(false);
                    show(item);
                  }}
                  className="block w-full px-3 py-1.5 text-left text-[12.5px] text-white/70 hover:bg-white/5"
                >
                  {item}
                </button>
              ))}
            </div>
          </>
        )}

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

      <span
        className="shrink-0 select-none font-mono text-[11.5px] font-semibold tracking-wide text-white/60"
      >
        OpenRize
      </span>

      <div className="flex flex-1 items-center justify-end">
        <button
          type="button"
          onClick={() => show("Refer friends")}
          className="rounded-lg border border-white/10 bg-white/5 px-2.5 py-1.5 text-[12.5px] font-medium text-white/75 hover:bg-white/10"
        >
          Refer friends
        </button>
      </div>
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
      className="grid size-7 shrink-0 place-items-center rounded-lg text-white/60 hover:bg-white/10 disabled:pointer-events-none disabled:text-white/20"
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
