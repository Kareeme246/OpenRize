import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { llmUnavailableReason } from "../hooks/useAiStatus";
import { describeError } from "../lib/api";
import type { AiStatus } from "../lib/types";

const APPLE_INTELLIGENCE_SETTINGS =
  "x-apple.systempreferences:com.apple.Siri-Settings.extension";

const DISMISS_KEY = "openrize.aiBannerDismissed";

function readDismissed(): string | null {
  try {
    return window.localStorage.getItem(DISMISS_KEY);
  } catch {
    return null;
  }
}

/**
 * Dismissible fallback shown when Apple Intelligence could be turned on and
 * suggestions run on rules alone.
 */
export function AiEngineBanner({ status }: { status: AiStatus | null }) {
  const [dismissed, setDismissed] = useState<string | null>(readDismissed);

  if (status === null || status.engine !== "fallback") return null;
  const reason = status.llmReason;
  const fixable =
    reason === "appleIntelligenceNotEnabled" || reason === "modelNotReady";
  if (!fixable || dismissed === reason) return null;

  const dismiss = (): void => {
    setDismissed(reason);
    try {
      window.localStorage.setItem(DISMISS_KEY, reason);
    } catch {
      // Private storage: the banner simply returns next launch.
    }
  };

  return (
    <div
      role="status"
      className="flex shrink-0 items-center gap-3 border-b border-line bg-surface px-5 py-2 text-[12px]"
    >
      <span className="size-1.5 shrink-0 rounded-full bg-accent" />
      <span className="min-w-0 flex-1 text-fg-muted">
        <span className="font-semibold text-fg">
          {llmUnavailableReason(status)}.
        </span>{" "}
        Suggestions use your rules and personal model until it's ready; entries
        are re-checked when it is.
      </span>
      {reason === "appleIntelligenceNotEnabled" && (
        <button
          type="button"
          onClick={() =>
            openUrl(APPLE_INTELLIGENCE_SETTINGS).catch((err: unknown) =>
              console.error(
                "Failed to open System Settings",
                describeError(err),
              ),
            )
          }
          className="shrink-0 rounded-md border border-line-strong px-2.5 py-1 text-[11.5px] font-semibold text-fg hover:bg-surface-strong"
        >
          Turn on in System Settings
        </button>
      )}
      <button
        type="button"
        onClick={dismiss}
        aria-label="Dismiss"
        className="shrink-0 rounded p-1 text-fg-faint hover:bg-surface hover:text-fg"
      >
        ✕
      </button>
    </div>
  );
}
