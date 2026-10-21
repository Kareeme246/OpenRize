import { useCallback, useEffect, useState } from "react";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { useToast } from "../hooks/useToast";
import { describePausedWork, type PausedOnSwitch } from "../lib/accounts";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { formatTime } from "../lib/format";

/**
 * Offers the work an account switch paused when the user left this account:
 * stopwatches that were running and a break that was on. It is never resumed
 * on its own; the cross keeps everything paused.
 */
export function PausedOnSwitchBanner() {
  const [paused, setPaused] = useState<PausedOnSwitch | null>(null);
  const [busy, setBusy] = useState(false);
  const showToast = useToast();

  const load = useCallback((): void => {
    api
      .pausedOnSwitch()
      .then(setPaused)
      .catch((cause: unknown) =>
        console.error("Could not read paused work", describeError(cause)),
      );
  }, []);
  useEffect(load, [load]);
  // Starting one of the stopwatches by hand takes it off the list.
  useTauriEvent(api.TIMERS_CHANGED, load);
  useTauriEvent(api.BREAK_STATE_CHANGED, load);

  if (paused === null) return null;

  const both = paused.stopwatches.length > 0 && paused.break !== null;
  const act = async (action: () => Promise<void>): Promise<void> => {
    setBusy(true);
    try {
      await action();
      setPaused(null);
    } catch (cause) {
      showToast({ tone: "error", message: describeError(cause) });
      load();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      role="status"
      className="flex shrink-0 items-center gap-3 border-b border-line bg-surface px-5 py-2 text-[12px]"
    >
      <span className="size-1.5 shrink-0 rounded-full bg-review" />
      <span className="min-w-0 flex-1 text-fg-muted">
        <span className="font-semibold text-fg">
          Paused when you switched accounts at {formatTime(paused.at)}:
        </span>{" "}
        {describePausedWork(paused.stopwatches, paused.break)}.
      </span>
      <button
        type="button"
        disabled={busy}
        onClick={() => void act(api.resumePaused)}
        className="shrink-0 rounded-md border border-line-strong px-2.5 py-1 font-semibold text-[11.5px] text-fg hover:bg-surface-strong disabled:opacity-40"
      >
        {both ? "Resume both" : "Resume"}
      </button>
      <button
        type="button"
        disabled={busy}
        onClick={() => void act(api.dismissPaused)}
        aria-label="Keep paused"
        className="shrink-0 rounded p-1 text-fg-faint hover:bg-surface hover:text-fg"
      >
        ✕
      </button>
    </div>
  );
}
