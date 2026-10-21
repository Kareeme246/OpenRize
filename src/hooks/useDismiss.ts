import { type RefObject, useEffect } from "react";

/**
 * Closes a popover on a press outside `ref` or on Escape, for as long as it
 * is `open`. The latest `onClose` is always called, so it need not be stable.
 */
export function useDismiss(
  open: boolean,
  ref: RefObject<HTMLElement | null>,
  onClose: () => void,
): void {
  useEffect(() => {
    if (!open) return;
    const pressed = (event: PointerEvent): void => {
      if (
        event.target instanceof Node &&
        !ref.current?.contains(event.target)
      ) {
        onClose();
      }
    };
    const key = (event: KeyboardEvent): void => {
      if (event.key === "Escape") onClose();
    };
    document.addEventListener("pointerdown", pressed);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", pressed);
      document.removeEventListener("keydown", key);
    };
  }, [open, ref, onClose]);
}
