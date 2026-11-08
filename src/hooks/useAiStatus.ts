import { useEffect, useState } from "react";
import * as api from "../lib/api";
import {
  EXPECTS_ON_DEVICE_ML,
  UNSUPPORTED_OS_MODEL_REASON,
} from "../lib/platform";
import type { AiStatus } from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

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
 * Whether this build has no model provider; the platform's expectation
 * stands in until the status settles, so controls don't flash.
 */
export function noLlm(status: AiStatus | null): boolean {
  if (status === null || status.engine === "starting") {
    return !EXPECTS_ON_DEVICE_ML;
  }
  return status.provider === "none";
}

export function llmUnavailableReason(status: AiStatus): string | null {
  if (status.llm === "ready") return null;
  switch (status.llmReason) {
    case "appleIntelligenceNotEnabled":
      return "Apple Intelligence is off";
    case "modelNotReady":
      return "The on-device model is still downloading";
    case "deviceNotEligible":
      return "This Mac doesn't support Apple Intelligence";
    case "unsupportedOS":
      return UNSUPPORTED_OS_MODEL_REASON;
    default:
      return "The on-device model isn't available";
  }
}
