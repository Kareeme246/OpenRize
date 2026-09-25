import { openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { llmUnavailableReason } from "../hooks/useAiStatus";
import { describeError } from "../lib/api";
import type { AiStatus } from "../lib/types";

/** Apple Intelligence & Siri pane of System Settings. */
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
 * The one dismissible fallback banner: shown when Apple Intelligence could
 * be turned on (or is still downloading) and suggestions are running on
 * rules and the personal model alone. A Mac that can never run the model
 * gets no nag here; Settings explains it instead.
 */
export function AiEngineBanner({ status }: { status: AiStatus | null }) {
  const [dismissed, setDismissed] = useState<string | null>(readDismissed);

  if (status === null || status.engine !== "fallback") return null;
  const fixable =
    status.llm === "appleIntelligenceNotEnabled" ||
    status.llm === "modelNotReady";
  if (!fixable || dismissed === status.llm) return null;

  const dismiss = (): void => {
    setDismissed(status.llm);
    try {
      window.localStorage.setItem(DISMISS_KEY, status.llm);
    } catch {
      // Private storage: the banner simply returns next launch.
    }
  };

  return (
    <div
      role="status"
      className="flex shrink-0 items-center gap-3 border-b border-review/30 bg-review/10 px-5 py-2 text-[12px]"
    >
      <span className="size-1.5 shrink-0 rounded-full bg-review" />
      <span className="min-w-0 flex-1 text-fg-muted">
        <span className="font-semibold text-fg">
          {llmUnavailableReason(status)}.
        </span>{" "}
        Suggestions use your rules and personal model until it's ready; entries
        are re-checked when it is.
      </span>
      {status.llm === "appleIntelligenceNotEnabled" && (
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
          className="shrink-0 rounded-md border border-review/40 px-2.5 py-1 text-[11.5px] font-semibold text-review hover:bg-review/15"
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
