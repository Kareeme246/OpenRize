import { formatDuration, type TimerGroup } from "../lib/timers";
import { BUTTON_SECONDARY } from "./Page";

interface TimerGroupBarProps {
  groups: TimerGroup[];
  onStart: (group: string) => void;
  onPause: (group: string) => void;
}

/** One row per group: its combined time, and Start all / Pause all. */
export function TimerGroupBar({
  groups,
  onStart,
  onPause,
}: TimerGroupBarProps) {
  if (groups.length === 0) return null;

  return (
    <section
      aria-label="Stopwatch groups"
      className="grid grid-cols-[repeat(auto-fill,minmax(520px,1fr))] gap-3"
    >
      {groups.map((group) => {
        const allRunning = group.running === group.timers;
        return (
          <div
            key={group.name}
            data-timer-group={group.name}
            className={`flex min-w-0 items-center gap-3 rounded-xl border border-settings-card-border bg-settings-card px-3.5 py-2.5 ${
              group.running > 0 ? "border-l-4 border-l-accent" : ""
            }`}
          >
            <div className="min-w-0 flex-1">
              <div className="truncate font-semibold text-[13px] text-fg-strong">
                {group.name}
              </div>
              <div className="truncate text-[11.5px] text-fg-faint">
                {group.timers}{" "}
                {group.timers === 1 ? "stopwatch" : "stopwatches"}
                {" · "}
                {group.running} running
              </div>
            </div>
            <div
              className={`font-mono font-semibold text-[16px] tabular-nums ${
                group.running > 0 ? "text-accent" : "text-fg-soft"
              }`}
            >
              {formatDuration(group.totalMs)}
            </div>
            <div className="flex shrink-0 gap-1.5">
              <button
                type="button"
                disabled={allRunning}
                aria-label={`Start all in ${group.name}`}
                onClick={() => onStart(group.name)}
                className={BUTTON_SECONDARY}
              >
                Start all
              </button>
              <button
                type="button"
                disabled={group.running === 0}
                aria-label={`Pause all in ${group.name}`}
                onClick={() => onPause(group.name)}
                className={BUTTON_SECONDARY}
              >
                Pause all
              </button>
            </div>
          </div>
        );
      })}
    </section>
  );
}
