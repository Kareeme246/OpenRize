import { type RefObject, useEffect } from "react";

/**
 * Closes a popover on an outside press or Escape while `open`; always calls the
 * latest `onClose`.
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
