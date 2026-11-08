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
 * Whether no language model is active (none chosen, or no provider installed); the platform's
 * expectation stands in until the status settles, so controls don't flash.
 */
export function noLlm(status: AiStatus | null): boolean {
  if (status === null || status.engine === "starting") {
    return !EXPECTS_ON_DEVICE_ML;
  }
  return status.provider === "none";
}

/**
 * Whether the build has no on-device ML at all, not even the personal model's embedder. A Mac
 * with the language model switched off still has the personal model.
 */
export function noOnDeviceMl(status: AiStatus | null): boolean {
  if (status === null || status.engine === "starting") {
    return !EXPECTS_ON_DEVICE_ML;
  }
  return status.provider === "none" && status.embed === "none";
}

/** Whether the personal model (the sidecar's embedder and classifier) can run. */
export function hasPersonalModel(status: AiStatus | null): boolean {
  if (status === null || status.engine === "starting") {
    return EXPECTS_ON_DEVICE_ML;
  }
  return status.provider === "apple" || status.embed === "ready";
}

/** What names the active language model in sentences. */
export function llmName(status: AiStatus | null): string {
  return status?.provider === "local"
    ? "a local model on this device"
    : "Apple's on-device model";
}

export function llmUnavailableReason(status: AiStatus): string | null {
  if (status.llm === "ready") return null;
  if (status.llm === "loading") return "The local model is loading";
  switch (status.llmReason) {
    case "appleIntelligenceNotEnabled":
      return "Apple Intelligence is off";
    case "modelNotReady":
      return "The on-device model is still downloading";
    case "deviceNotEligible":
      return "This Mac doesn't support Apple Intelligence";
    case "unsupportedOS":
      return UNSUPPORTED_OS_MODEL_REASON;
    case "modelMissing":
      return "The selected local model file isn't in the models folder";
    case "runtimeMissing":
      return "The local model runtime isn't installed with this build";
    case "runtimeFailed":
      return "The local model couldn't start, on the GPU or the CPU";
    default:
      return status.provider === "none" && status.llmReason === undefined
        ? "No language model is selected"
        : "The on-device model isn't available";
  }
}
