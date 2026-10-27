/// <reference types="vite/client" />
import type { AccessibilityState } from "./types";

/**
 * Developer tools ship only in a dev build; the backend checks again
 * (src-tauri/src/dev_tools.rs). Only Vite's dev server sets `import.meta.env.DEV`.
 */
export const DEV_TOOLS: boolean = import.meta.env?.DEV === true;

/** The picker keys for Accessibility: the real state, or one of the simulated ones. */
export type AccessibilityChoice =
  | "real"
  | "notGranted"
  | "checking"
  | "notWorking"
  | "relaunchRequired"
  | "working";

/** The picker keys for Automation: the real grant, or a simulated one. */
export type AutomationChoice = "real" | "missing" | "allowed";

/** What a simulated (or real) Accessibility state shows as in the picker. */
export function accessibilityChoice(
  state: AccessibilityState | null,
): AccessibilityChoice {
  if (state === null || state === "notApplicable") return "real";
  return state;
}

/** The simulation a picker key asks for; `null` means the real state. */
export function accessibilityFromChoice(
  choice: AccessibilityChoice,
): AccessibilityState | null {
  return choice === "real" ? null : choice;
}

export function automationChoice(granted: boolean | null): AutomationChoice {
  if (granted === null) return "real";
  return granted ? "allowed" : "missing";
}

/** The simulation a picker key asks for; `null` means the real grant. */
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

/** The command that revokes this build's real Accessibility grant, per app identity. */
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
