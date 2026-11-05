import type { AccessibilityState } from "./types";

export const ACCESSIBILITY_ID = "accessibility";

export function stateLabel(state: AccessibilityState): string {
  switch (state) {
    case "working":
      return "Granted";
    case "checking":
      return "Checking…";
    case "notWorking":
      return "Allowed, but not working";
    case "relaunchRequired":
      return "Relaunch needed";
    case "notGranted":
    case "notApplicable":
      return "Not granted";
  }
}

export type StateTone = "success" | "danger" | "warning" | "muted";

export const TONE_CLASS: Record<StateTone, string> = {
  success: "text-success",
  danger: "text-danger",
  warning: "text-amber-500",
  muted: "text-fg-muted",
};

export function stateTone(state: AccessibilityState): StateTone {
  switch (state) {
    case "working":
      return "success";
    case "notGranted":
      return "danger";
    case "notWorking":
    case "relaunchRequired":
      return "warning";
    case "checking":
    case "notApplicable":
      return "muted";
  }
}

/**
 * Accessibility is recommended, not required: OpenRize tracks apps without it,
 * but window titles sharpen categories and projects.
 */
export function roleLabel(id: string, required: boolean): string {
  if (required) return "Required";
  return id === ACCESSIBILITY_ID ? "Recommended" : "Optional";
}
