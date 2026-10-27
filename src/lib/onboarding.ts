import { type StateTone, stateLabel, stateTone } from "./permissions.ts";
import type { AccessibilityState } from "./types";

/** What the first-run permissions screen is showing, if anything. */
export type OnboardingStep =
  | "hidden"
  /** Accessibility is off: the screen offers it, and skipping is fine. */
  | "asking"
  /** Allowed; waiting to hear whether it works in this running copy. */
  | "waiting"
  /** Allowed and working. */
  | "granted"
  /** Allowed, but not working. Settings explains what to do. */
  | "notWorking"
  /** Allowed while this copy ran, and only a restart makes it work. */
  | "relaunch";

export interface OnboardingInput {
  /** The user has not skipped or finished it before; `null` until known. */
  needed: boolean | null;
  /** The screen has already come up in this session. */
  shown: boolean;
  accessibility: AccessibilityState | null;
}

/**
 * Which step to show. It only ever opens while Accessibility is off, so an
 * install where it already works, or a platform that has no such permission,
 * never sees it. Once it is up it stays, following the state, until the user
 * dismisses it: allowing access must not make the screen vanish before it has
 * said so. When a restart is needed it stays up and its one button becomes
 * Restart (`onboardingAction`); the relaunch screen is not shown over it
 * (`relaunchModalOpen`).
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

/** The screen's one primary button. */
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
      /** Said once, above the button. */
      note: string;
    };

/**
 * Continue, except once a grant has been made that only a restart completes:
 * then the same button is the restart, and nothing else on the screen leaves
 * it. While a fresh grant is still being checked Continue waits, so there is
 * no moment between the grant and the answer where it can be pressed. Only the
 * Accessibility state decides this; Automation takes effect at once and never
 * asks for a restart, and not granting Accessibility leaves Continue alone.
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
 * Whether the relaunch screen is up. Onboarding owns the relaunch while it is
 * showing, so the two are never up together; everywhere else the relaunch
 * screen is how a grant that needs a restart is announced.
 */
export function relaunchModalOpen(
  accessibility: AccessibilityState | null,
  step: OnboardingStep,
): boolean {
  return accessibility === "relaunchRequired" && step !== "relaunch";
}

/** One permission card's status line and whether it still offers to grant. */
export interface PermissionCardView {
  status: string;
  tone: StateTone;
  /** Only while the permission is not granted; once it is, there is nothing to press. */
  showGrant: boolean;
}

/** The Accessibility card follows the step, so it keeps up as macOS applies the grant. */
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
