import { useEffect, useRef, useState } from "react";
import { MIN_HALF_MS } from "../../components/SplitDialog";
import { formatTime } from "../../lib/format";
import {
  hourOffset,
  type Placement,
  type Timeline,
  timeAtOffset,
} from "./timeline";

const MINUTE = 60_000;

interface SplitPickerProps {
  /** The entry block's box, as `place` gave it. */
  top: number;
  height: number;
  narrow?: boolean;
  startedAt: number;
  /** The entry's end; for the live entry, now. */
  endsAt: number;
  /** Where the cut starts out (the smart split point). */
  initialAt: number;
  timeline: Timeline;
  dayStart: number;
  placement: Placement;
  onPick: (at: number) => void;
}

/**
 * Split mode on the selected entry: a cut line that follows the pointer over
 * the block, snapped to the minute, and a click (or Enter) that takes it to
 * the split dialog. ↑/↓ nudge it a minute (⇧ for five); Esc, handled by the
 * review sheet, leaves split mode.
 */
export function SplitPicker({
  top,
  height,
  narrow = false,
  startedAt,
  endsAt,
  initialAt,
  timeline,
  dayStart,
  placement,
  onPick,
}: SplitPickerProps) {
  const ref = useRef<HTMLButtonElement>(null);
  const lowest = Math.ceil((startedAt + MIN_HALF_MS) / MINUTE) * MINUTE;
  const highest = Math.floor((endsAt - MIN_HALF_MS) / MINUTE) * MINUTE;
  const clamp = (time: number): number =>
    Math.min(highest, Math.max(lowest, time));
  const [at, setAt] = useState(() => clamp(initialAt));

  useEffect(() => {
    ref.current?.focus({ preventScroll: true });
  }, []);

  const lineY = Math.min(
    height,
    Math.max(
      0,
      (hourOffset(at, dayStart, placement) - timeline.startHour) *
        timeline.hourHeight -
        top,
    ),
  );
  /** Moves the cut under the pointer; returns where it landed. */
  const follow = (clientY: number): number => {
    const box = ref.current?.getBoundingClientRect();
    if (!box) return at;
    const time = timeAtOffset(
      timeline,
      top + clientY - box.top,
      dayStart,
      placement,
    );
    const snapped = clamp(Math.round(time / MINUTE) * MINUTE);
    setAt(snapped);
    return snapped;
  };

  return (
    <button
      ref={ref}
      type="button"
      aria-label={`Split here, at ${formatTime(at)}`}
      className={`calendar-entry absolute z-30 cursor-row-resize rounded-md outline-none focus-visible:ring-2 focus-visible:ring-accent ${
        narrow ? "right-1 left-0.5" : "right-4 left-0"
      }`}
      style={{ top: `${top}px`, height: `${height}px` }}
      onPointerMove={(event) => follow(event.clientY)}
      onClick={(event) => onPick(follow(event.clientY))}
      onKeyDown={(event) => {
        if (event.key === "ArrowUp" || event.key === "ArrowDown") {
          event.preventDefault();
          const step = (event.shiftKey ? 5 : 1) * MINUTE;
          setAt((current) =>
            clamp(current + (event.key === "ArrowUp" ? -step : step)),
          );
        } else if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onPick(at);
        }
      }}
    >
      <span
        className="pointer-events-none absolute right-0 left-0 flex -translate-y-1/2 items-center"
        style={{ top: `${lineY}px` }}
      >
        <span className="h-0 flex-1 border-accent border-t-2 border-dashed" />
        <span className="ml-1 flex shrink-0 items-center gap-1 rounded-full bg-accent px-1.5 py-0.5 font-semibold text-[10px] text-accent-fg leading-none tabular-nums shadow-sm">
          <span aria-hidden="true">✂</span>
          {formatTime(at)}
        </span>
      </span>
    </button>
  );
}
