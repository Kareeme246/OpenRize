import type { AccessibilityState } from "./types";

export const ACCESSIBILITY_ID = "accessibility";

/** What the user is told about a permission's state, in one phrase. */
export function stateLabel(state: AccessibilityState): string {
  switch (state) {
    case "working":
      return "On";
    case "checking":
      return "Checking…";
    case "notWorking":
      return "Allowed, but not working";
    case "relaunchRequired":
      return "Relaunch needed";
    case "notGranted":
    case "notApplicable":
      return "Off";
  }
}
