import { type StateTone, stateLabel, stateTone } from "./permissions.ts";
import type { AccessibilityState } from "./types";

export type OnboardingStep =
  | "hidden"
  | "asking"
  /** Allowed; waiting to hear whether it works in this running copy. */
  | "waiting"
  | "granted"
  | "notWorking"
  /** Allowed while this copy ran, and only a restart makes it work. */
  | "relaunch";

export interface OnboardingInput {
  /** The user has not skipped or finished it before; `null` until known. */
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
      /** A grant is being checked: it may turn out to need a restart, so leaving waits for the answer. */
      disabled: boolean;
    }
  | {
      kind: "restart";
      label: "Restart required";
      disabled: false;
      note: string;
    };

/**
 * Continue, except once a grant needs a restart: then it is the restart. Waits
 * while a grant is checked.
 */
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

/**
 * Whether the relaunch screen is up; onboarding owns the relaunch while it
 * shows.
 */
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

/** Automation is a yes or no; `undefined` is not known yet, so the card says nothing. */
export function automationCard(
  granted: boolean | undefined,
): PermissionCardView | null {
  if (granted === undefined) return null;
  return cardFor(granted ? "working" : "notGranted");
}

function cardFor(state: AccessibilityState): PermissionCardView {
  return {
    // The button on this screen says Restart, so the card does too.
    status: state === "relaunchRequired" ? "Restart needed" : stateLabel(state),
    tone: stateTone(state),
    showGrant: state === "notGranted",
  };
}
