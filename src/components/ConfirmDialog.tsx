import { useEffect, useRef } from "react";

interface ConfirmDialogProps {
  title: string;
  body: string;
  confirmLabel: string;
  /**
   * `destructive` (the default) makes Cancel the primary button;
   * `affirmative` (e.g. "Approve 4 entries?") makes the confirm button it.
   */
  tone?: "destructive" | "affirmative";
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
  tone = "destructive",
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const primary =
    "rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3 py-1.5 text-[12.5px] font-semibold text-accent-fg";
  const secondary =
    "rounded-lg border border-line bg-surface px-3 py-1.5 text-[12.5px] font-medium text-fg-muted hover:bg-surface-strong";
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
    // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop click-to-cancel; native onCancel (Escape) already provides the keyboard equivalent
    <dialog
      ref={ref}
      tabIndex={-1}
      onCancel={onCancel}
      onClick={(event) => {
        if (event.target === event.currentTarget) onCancel();
      }}
      className="m-auto rounded-2xl bg-transparent p-0 text-fg-strong outline-none backdrop:bg-scrim"
    >
      <div className="w-[340px] max-w-[calc(100vw-2rem)] rounded-2xl border border-line bg-panel p-4 shadow-2xl">
        <h2 className="text-[13.5px] font-semibold">{title}</h2>
        <p className="mt-1.5 text-[12.5px] leading-relaxed text-fg-soft">
          {body}
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className={tone === "destructive" ? primary : secondary}
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className={tone === "destructive" ? secondary : primary}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </dialog>
  );
}
