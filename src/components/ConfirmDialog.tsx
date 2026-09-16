import { useEffect, useRef } from "react";

interface ConfirmDialogProps {
  title: string;
  body: string;
  confirmLabel: string;
  /** Red confirm button, for actions that throw data away. */
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * A real <dialog> opened with showModal(), so the browser hands us the top
 * layer, the focus trap and Escape-to-cancel for free. Two details matter:
 *
 * - No padding or background of its own; the panel is the inner div. That makes
 *   the dialog's box exactly the panel's box, so a click whose target *is* the
 *   dialog landed on ::backdrop — i.e. the greyed-out area — and closes it.
 * - `m-auto` is load-bearing. Preflight's `* { margin: 0 }` is an author style
 *   and so beats the UA's `dialog { margin: auto }`, which would otherwise pin
 *   the dialog to the top-left instead of centring it.
 */
export function ConfirmDialog({
  title,
  body,
  confirmLabel,
  danger = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) return;
    dialog.showModal();
    return () => dialog.close();
  }, []);

  return (
    <dialog
      ref={ref}
      onCancel={onCancel}
      onClick={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
      className="m-auto rounded-2xl bg-transparent p-0 text-white backdrop:bg-black/65"
    >
      <div className="w-[340px] max-w-[calc(100vw-2rem)] rounded-2xl border border-white/10 bg-ink-900 p-4 shadow-2xl">
        <h2 className="text-[13.5px] font-semibold">{title}</h2>
        <p className="mt-1.5 text-[12.5px] leading-relaxed text-white/50">
          {body}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          {/* First focusable child, so showModal() puts initial focus here. */}
          <button
            type="button"
            onClick={onCancel}
            className="rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12.5px] font-medium text-white/80"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className={`rounded-lg border px-3 py-1.5 text-[12.5px] font-semibold ${
              danger
                ? "border-red-400/40 bg-red-400/15 text-red-200"
                : "border-accent/30 bg-accent-soft text-accent"
            }`}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </dialog>
  );
}
