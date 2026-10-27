import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { type OnboardingStep, onboardingStep } from "../lib/onboarding";
import type { AccessibilityState } from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

/**
 * Which step of the first-run permissions screen is up, and how to leave it.
 * Kept above the screen so the relaunch screen can ask whether this one has
 * taken the relaunch over (see `relaunchModalOpen`).
 */
export function useOnboarding(accessibility: AccessibilityState | null): {
  step: OnboardingStep;
  /** Marks it done and closes it. */
  finish: () => void;
} {
  const [needed, setNeeded] = useState<boolean | null>(null);
  const [shown, setShown] = useState(false);
  const step = onboardingStep({ needed, shown, accessibility });

  const readNeeded = useCallback(() => {
    api
      .onboardingNeeded()
      .then(setNeeded)
      .catch((cause: unknown) =>
        console.error("Could not read onboarding", describeError(cause)),
      );
  }, []);
  useEffect(readNeeded, [readNeeded]);
  // A developer re-run brings the screen back at once, even if it was shown before.
  useTauriEvent(api.ONBOARDING_CHANGED, () => {
    readNeeded();
    setShown(false);
  });
  useEffect(() => {
    if (step !== "hidden") setShown(true);
  }, [step]);

  const finish = useCallback(() => {
    setNeeded(false);
    api
      .finishOnboarding()
      .catch((cause: unknown) =>
        console.error("Could not save onboarding", describeError(cause)),
      );
  }, []);

  return { step, finish };
}
