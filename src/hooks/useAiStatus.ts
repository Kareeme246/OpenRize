import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";
import * as api from "../lib/api";
import type { AiStatus } from "../lib/types";

/** The AI engine status, kept live by `ai-status-changed`. */
export function useAiStatus(): AiStatus | null {
  const [status, setStatus] = useState<AiStatus | null>(null);

  useEffect(() => {
    let active = true;
    api
      .aiStatus()
      .then((loaded) => {
        if (active) setStatus(loaded);
      })
      .catch((err: unknown) =>
        console.error("Failed to load AI status", api.describeError(err)),
      );
    const unlisten = listen<AiStatus>(api.AI_STATUS_CHANGED, (event) =>
      setStatus(event.payload),
    );
    return () => {
      active = false;
      void unlisten.then((stop) => stop());
    };
  }, []);

  return status;
}

/** Human description of why the Foundation Model isn't in use. */
export function llmUnavailableReason(status: AiStatus): string | null {
  switch (status.llm) {
    case "available":
      return null;
    case "appleIntelligenceNotEnabled":
      return "Apple Intelligence is off";
    case "modelNotReady":
      return "The on-device model is still downloading";
    case "deviceNotEligible":
      return "This Mac doesn't support Apple Intelligence";
    case "unsupportedOS":
      return "The on-device model needs macOS 26 or later";
    default:
      return "The on-device model isn't available";
  }
}
