import { type StateTone, stateLabel, stateTone } from "./permissions.ts";
import type { AccessibilityState } from "./types";

export type OnboardingStep =
  | "hidden"
  | "asking"
  | "waiting"
  | "granted"
  | "notWorking"
  /** Allowed while this copy ran, and only a restart makes it work. */
  | "relaunch";

export interface OnboardingInput {
  needed: boolean | null;
  shown: boolean;
  accessibility: AccessibilityState | null;
}

/**
 * Opens only while Accessibility is off, then follows the state until
 * dismissed; a needed restart hides the relaunch screen (`relaunchModalOpen`).
 */
export function onboardingStep({
  needed,
  shown,
  accessibility,
}: OnboardingInput): OnboardingStep {
  if (needed !== true || accessibility === null) return "hidden";
  switch (accessibility) {
    case "notGranted":
      return "asking";
    case "checking":
      return shown ? "waiting" : "hidden";
    case "working":
      return shown ? "granted" : "hidden";
    case "notWorking":
      return shown ? "notWorking" : "hidden";
    case "relaunchRequired":
      return "relaunch";
    case "notApplicable":
      return "hidden";
  }
}

export type OnboardingAction =
  | {
      kind: "continue";
      label: "Continue";
      disabled: boolean;
    }
  | {
      kind: "restart";
      label: "Restart required";
      disabled: false;
      note: string;
    };

export function onboardingAction(
  step: Exclude<OnboardingStep, "hidden">,
): OnboardingAction {
  if (step === "relaunch") {
    return {
      kind: "restart",
      label: "Restart required",
      disabled: false,
      note: "Accessibility is allowed. Restart OpenRize to finish.",
    };
  }
  return { kind: "continue", label: "Continue", disabled: step === "waiting" };
}

export function relaunchModalOpen(
  accessibility: AccessibilityState | null,
  step: OnboardingStep,
): boolean {
  return accessibility === "relaunchRequired" && step !== "relaunch";
}

export interface PermissionCardView {
  status: string;
  tone: StateTone;
  showGrant: boolean;
}

export function accessibilityCard(
  step: Exclude<OnboardingStep, "hidden">,
): PermissionCardView {
  const states: Record<typeof step, AccessibilityState> = {
    asking: "notGranted",
    waiting: "checking",
    granted: "working",
    notWorking: "notWorking",
    relaunch: "relaunchRequired",
  };
  return cardFor(states[step]);
}

export function automationCard(
  granted: boolean | undefined,
): PermissionCardView | null {
  if (granted === undefined) return null;
  return cardFor(granted ? "working" : "notGranted");
}

function cardFor(state: AccessibilityState): PermissionCardView {
  return {
    status: state === "relaunchRequired" ? "Restart needed" : stateLabel(state),
    tone: stateTone(state),
    showGrant: state === "notGranted",
  };
}
