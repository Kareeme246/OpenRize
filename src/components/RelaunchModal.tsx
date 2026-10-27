import { useEffect, useRef, useState } from "react";
import { useAccessibilityState } from "../hooks/useAccessibilityState";
import * as api from "../lib/api";

/**
 * Held over the whole window from the moment Accessibility is allowed in a
 * running OpenRize that still cannot use it, until the app restarts. macOS
 * only gives such a process the access on its next launch, so there is nothing
 * to dismiss and nothing to postpone: no close button, no "later". If the
 * access is withdrawn or starts working on its own, the backend says so and the
 * screen goes away by itself.
 */
export function RelaunchModal() {
  const state = useAccessibilityState();
  if (state !== "relaunchRequired") return null;
  return <Screen />;
}

function Screen() {
  const ref = useRef<HTMLDialogElement>(null);
  const [relaunching, setRelaunching] = useState(false);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) return;
    dialog.showModal();
    return () => dialog.close();
  }, []);

  const relaunch = (): void => {
    setRelaunching(true);
    setFailed(false);
    api.relaunchApp().catch((cause: unknown) => {
      console.error("Could not relaunch", api.describeError(cause));
      setRelaunching(false);
      setFailed(true);
    });
  };

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
          Relaunch OpenRize to finish setup
        </h2>
        <p
          id="relaunch-body"
          className="mt-1.5 text-[12.5px] leading-relaxed text-fg-soft"
        >
          Accessibility is allowed, but macOS only gives it to OpenRize the next
          time it opens. Until then window titles can't be read. Relaunching
          takes a moment and keeps your data and trackers.
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
