/// <reference types="vite/client" />
import type { AccessibilityState } from "./types";

/** Dev builds only; the backend checks again (src-tauri/src/dev_tools.rs). */
export const DEV_TOOLS: boolean = import.meta.env?.DEV === true;

export type AccessibilityChoice =
  | "real"
  | "notGranted"
  | "checking"
  | "notWorking"
  | "relaunchRequired"
  | "working";

export type AutomationChoice = "real" | "missing" | "allowed";

export function accessibilityChoice(
  state: AccessibilityState | null,
): AccessibilityChoice {
  if (state === null || state === "notApplicable") return "real";
  return state;
}

export function accessibilityFromChoice(
  choice: AccessibilityChoice,
): AccessibilityState | null {
  return choice === "real" ? null : choice;
}

export function automationChoice(granted: boolean | null): AutomationChoice {
  if (granted === null) return "real";
  return granted ? "allowed" : "missing";
}

export function automationFromChoice(choice: AutomationChoice): boolean | null {
  switch (choice) {
    case "real":
      return null;
    case "missing":
      return false;
    case "allowed":
      return true;
  }
}

export function tccResetCommand(identifier: string): string {
  return `tccutil reset Accessibility ${identifier}`;
}

/** Whether `identifier` is the installed app's, whose grant a dev build must never touch. */
export function isReleaseIdentifier(
  identifier: string,
  releaseIdentifier: string,
): boolean {
  return identifier === releaseIdentifier;
}
