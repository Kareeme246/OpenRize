import { useEffect, useState } from "react";
import * as api from "../lib/api";
import type { AccessibilityState } from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

/**
 * The backend's view of Accessibility, kept live by `permissions-changed`.
 * `null` until the first answer, so nothing is claimed before it arrives.
 */
export function useAccessibilityState(): AccessibilityState | null {
  const [state, setState] = useState<AccessibilityState | null>(null);

  useEffect(() => {
    let active = true;
    api
      .accessibilityState()
      .then((loaded) => {
        if (active) setState(loaded);
      })
      .catch((err: unknown) =>
        console.error(
          "Failed to load the Accessibility state",
          api.describeError(err),
        ),
      );
    return () => {
      active = false;
    };
  }, []);
  useTauriEvent<AccessibilityState>(api.PERMISSIONS_CHANGED, setState);

  return state;
}
