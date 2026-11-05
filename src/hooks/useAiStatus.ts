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
 * Whether the on-device ML sidecar is in this build; the platform's expectation
 * stands in until the status settles, so controls don't flash.
 */
export function onDeviceMlUnavailable(status: AiStatus | null): boolean {
  if (status === null || status.engine === "starting") {
    return !EXPECTS_ON_DEVICE_ML;
  }
  return status.sidecar === "unavailable";
}

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
      return UNSUPPORTED_OS_MODEL_REASON;
    default:
      return "The on-device model isn't available";
  }
}
