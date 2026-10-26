import { useEffect, useState } from "react";
import * as api from "../lib/api";
import { EXPECTS_ON_DEVICE_ML } from "../lib/platform";
import type { AiStatus } from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

/** The AI engine status, kept live by `ai-status-changed`. */
export function useAiStatus(): AiStatus | null {
  const [status, setStatus] = useState<AiStatus | null>(null);

  useEffect(() => {
    let active = true;
    const refresh = (): void => {
      api
        .aiStatus()
        .then((loaded) => {
          if (active) setStatus(loaded);
        })
        .catch((err: unknown) =>
          console.error("Failed to load AI status", api.describeError(err)),
        );
    };
    refresh();
    window.addEventListener("focus", refresh);
    return () => {
      active = false;
      window.removeEventListener("focus", refresh);
    };
  }, []);
  useTauriEvent<AiStatus>(api.AI_STATUS_CHANGED, setStatus);

  return status;
}

/**
 * The on-device ML (sidecar) isn't in this build, so suggestions come from the
 * rules alone and anything that configures or trains a model does nothing.
 * Read from the status so a platform that later ships a model lights these
 * controls back up; until the status settles, the platform's expectation
 * stands in so the controls don't flash in and out.
 */
export function onDeviceMlUnavailable(status: AiStatus | null): boolean {
  if (status === null || status.engine === "starting") {
    return !EXPECTS_ON_DEVICE_ML;
  }
  return status.sidecar === "unavailable";
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
