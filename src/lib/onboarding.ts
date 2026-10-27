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
  | "notWorking";

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
 * said so. When a relaunch is needed the relaunch screen is the only thing on
 * screen, so this one steps aside.
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
    case "notApplicable":
      return "hidden";
  }
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
    status: stateLabel(state),
    tone: stateTone(state),
    showGrant: state === "notGranted",
  };
}
