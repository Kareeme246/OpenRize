import { useEffect, useRef } from "react";

interface ConfirmDialogProps {
  title: string;
  body: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * A real <dialog> opened with showModal(), so the browser hands us the top
 * layer, the focus trap and Escape-to-cancel for free. Three details matter:
 *

 */
export function ConfirmDialog({
  title,
  body,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) return;
    dialog.showModal();
    // showModal() has just focused the first child; move focus to the dialog so
    // neither button reads as chosen. Needs the tabIndex below to be focusable.
    dialog.focus();
    return () => dialog.close();
  }, []);

  return (
    <dialog
      ref={ref}
      tabIndex={-1}
      onCancel={onCancel}
      onClick={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
      className="m-auto rounded-2xl bg-transparent p-0 text-white outline-none backdrop:bg-black/65"
    >
      <div className="w-[340px] max-w-[calc(100vw-2rem)] rounded-2xl border border-white/10 bg-ink-900 p-4 shadow-2xl">
        <h2 className="text-[13.5px] font-semibold">{title}</h2>
        <p className="mt-1.5 text-[12.5px] leading-relaxed text-white/50">
          {body}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3 py-1.5 text-[12.5px] font-semibold text-[#04160c]"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12.5px] font-medium text-white/70 hover:bg-white/10"
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </dialog>
  );
}
