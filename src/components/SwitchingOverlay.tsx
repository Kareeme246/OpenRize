import { useEffect, useRef } from "react";
import { useAccounts } from "../hooks/useAccounts";

/**
 * The screen held over the whole window from the moment an account switch is
 * asked for until the app restarts (at least three seconds, by the backend's
 * clock). A modal <dialog>, so nothing underneath can be reached meanwhile;
 * Escape does nothing, because there is nothing to cancel once it is under way.
 */
export function SwitchingOverlay() {
  const { switching } = useAccounts();
  if (switching === null) return null;
  return <Screen name={switching.name} />;
}

function Screen({ name }: { name: string | null }) {
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
      aria-live="polite"
      onCancel={(event) => event.preventDefault()}
      className="m-auto rounded-2xl bg-transparent p-0 text-fg-strong outline-none backdrop:bg-scrim backdrop:backdrop-blur-[2px]"
    >
      <div className="flex items-center gap-3 rounded-2xl border border-line bg-panel px-5 py-4 shadow-2xl">
        <span
          aria-hidden="true"
          className="size-4 animate-spin rounded-full border-2 border-line-strong border-t-accent"
        />
        <span className="font-semibold text-[13px] text-fg-strong">
          {name === null ? "Signing out..." : `Switching to ${name}...`}
        </span>
      </div>
    </dialog>
  );
}
