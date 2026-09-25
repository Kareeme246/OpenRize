import { type FormEvent, type ReactNode, useEffect, useRef } from "react";
import { BUTTON_PRIMARY, BUTTON_SECONDARY } from "./Page";

interface SheetProps {
  title: string;
  submitLabel: string;
  onSubmit: () => void;
  onClose: () => void;
  /** Disables submit, e.g. while a required field is empty. */
  canSubmit?: boolean;
  error?: string | null;
  width?: "sm" | "md" | "lg";
  children: ReactNode;
}

const WIDTHS = { sm: "w-[380px]", md: "w-[480px]", lg: "w-[600px]" };

/**
 * A modal form (Add time, New project, New client) on a real <dialog>, so
 * the browser provides the top layer, focus trap, and Escape to cancel, the
 * same way ConfirmDialog does.
 */
export function Sheet({
  title,
  submitLabel,
  onSubmit,
  onClose,
  canSubmit = true,
  error,
  width = "md",
  children,
}: SheetProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) return;
    dialog.showModal();
    return () => dialog.close();
  }, []);

  const submit = (event: FormEvent): void => {
    event.preventDefault();
    if (canSubmit) onSubmit();
  };

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop click-to-cancel; native onCancel (Escape) already provides the keyboard equivalent
    <dialog
      ref={ref}
      onCancel={(event) => {
        event.preventDefault();
        onClose();
      }}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
      className="m-auto rounded-2xl bg-transparent p-0 text-fg outline-none backdrop:bg-scrim"
    >
      <form
        onSubmit={submit}
        className={`${WIDTHS[width]} flex max-h-[min(720px,calc(100vh-4rem))] max-w-[calc(100vw-2rem)] flex-col rounded-2xl border border-line bg-panel shadow-2xl`}
      >
        <div className="flex items-center justify-between border-line border-b px-5 py-3">
          <h2 className="font-semibold text-[14px] text-fg-strong">{title}</h2>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="rounded p-1 text-fg-faint hover:bg-surface hover:text-fg"
          >
            ✕
          </button>
        </div>
        <div className="min-h-0 flex-1 space-y-3.5 overflow-y-auto px-5 py-4 text-[12px]">
          {children}
        </div>
        {error && (
          <div
            role="alert"
            className="mx-5 mb-2 rounded-md border border-danger/30 bg-danger-soft px-3 py-2 text-[11.5px] text-danger"
          >
            {error}
          </div>
        )}
        <div className="flex justify-end gap-2 border-line border-t px-5 py-3">
          <button type="button" onClick={onClose} className={BUTTON_SECONDARY}>
            Cancel
          </button>
          <button
            type="submit"
            disabled={!canSubmit}
            className={BUTTON_PRIMARY}
          >
            {submitLabel}
          </button>
        </div>
      </form>
    </dialog>
  );
}

/** A labelled form row. */
export function Field({
  label,
  htmlFor,
  hint,
  children,
}: {
  label: string;
  htmlFor?: string;
  hint?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div>
      <label
        htmlFor={htmlFor}
        className="mb-1 block font-medium text-[11.5px] text-fg-soft"
      >
        {label}
      </label>
      {children}
      {hint && <div className="mt-1 text-[11px] text-fg-faint">{hint}</div>}
    </div>
  );
}
