import { useEffect, useRef } from "react";
import { useRelaunch } from "../hooks/useRelaunch";

/**
 * Held over the whole window from the moment Accessibility is allowed in a
 * running OpenRize that still cannot use it, until the app restarts. macOS
 * only gives such a process the access on its next launch, so there is nothing
 * to dismiss and nothing to postpone: no close button, no "later". If the
 * access is withdrawn or starts working on its own, the backend says so and the
 * screen goes away by itself. While the first-run permissions screen is up it
 * asks for the restart itself, so `open` is false then (`relaunchModalOpen`).
 */
export function RelaunchModal({ open }: { open: boolean }) {
  if (!open) return null;
  return <Screen />;
}

function Screen() {
  const ref = useRef<HTMLDialogElement>(null);
  const { relaunching, failed, relaunch } = useRelaunch();

  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) return;
    dialog.showModal();
    return () => dialog.close();
  }, []);

  return (
    <dialog
      ref={ref}
      aria-labelledby="relaunch-title"
      aria-describedby="relaunch-body"
      // Escape is a cancel request; refusing it is the whole point.
      onCancel={(event) => event.preventDefault()}
      // Some engines close a modal whose cancel keeps being refused. This one
      // stays until the app restarts, so a close that was not ours is undone.
      onClose={(event) => {
        if (!event.currentTarget.open) event.currentTarget.showModal();
      }}
      className="m-auto rounded-2xl bg-transparent p-0 text-fg-strong outline-none backdrop:bg-scrim backdrop:backdrop-blur-[2px]"
    >
      <div className="w-[380px] max-w-[calc(100vw-2rem)] rounded-2xl border border-line bg-panel p-5 shadow-2xl">
        <h2 id="relaunch-title" className="text-[14px] font-semibold">
          Relaunch OpenRize
        </h2>
        <p
          id="relaunch-body"
          className="mt-1.5 text-[12.5px] leading-relaxed text-fg-soft"
        >
          Accessibility is allowed. Relaunch to finish.
        </p>
        {failed && (
          <p className="mt-2 text-[12px] text-rose-500" role="alert">
            OpenRize couldn't restart itself. Quit it from the menu bar and open
            it again.
          </p>
        )}
        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={relaunch}
            disabled={relaunching}
            // biome-ignore lint/a11y/noAutofocus: the only control on a screen that cannot be left
            autoFocus
            className="rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3.5 py-1.5 text-[12.5px] font-semibold text-accent-fg disabled:opacity-60"
          >
            {relaunching ? "Relaunching…" : "Relaunch now"}
          </button>
        </div>
      </div>
    </dialog>
  );
}
