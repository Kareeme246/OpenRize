const SIZE = 68;
const STROKE = 5;
const RADIUS = (SIZE - STROKE) / 2;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
const CENTRE = SIZE / 2;

/** The dial's cycle: one full turn per tracked hour, then it wraps. */
const HOUR_MS = 3_600_000;

interface HourRingProps {
  /** This tracker's own elapsed time in ms — not the wall clock. */
  elapsed: number;
  running: boolean;
}

/** Fraction of the current tracked hour that has passed, 0–1. Zero at 0:00,
 *  a quarter at 15:00, wrapping back to zero at 1:00:00. */
export function hourProgress(elapsed: number): number {
  return (elapsed % HOUR_MS) / HOUR_MS;
}

/** A dial that sweeps once per tracked hour. Its arc follows the timer, so a
 *  fresh tracker starts empty and the fill survives pause/resume. */
export function HourRing({ elapsed, running }: HourRingProps) {
  const progress = hourProgress(elapsed);
  const minutes = Math.floor((elapsed % HOUR_MS) / 60_000);

  return (
    <svg
      viewBox={`0 0 ${SIZE} ${SIZE}`}
      className="size-17 shrink-0"
      role="img"
      aria-label={`${minutes} minutes into this tracker's current hour`}
    >
      <circle
        cx={CENTRE}
        cy={CENTRE}
        r={RADIUS}
        fill="none"
        strokeWidth={STROKE}
        className="stroke-white/10"
      />
      <circle
        cx={CENTRE}
        cy={CENTRE}
        r={RADIUS}
        fill="none"
        strokeWidth={STROKE}
        strokeLinecap="round"
        strokeDasharray={CIRCUMFERENCE}
        strokeDashoffset={CIRCUMFERENCE * (1 - progress)}
        transform={`rotate(-90 ${CENTRE} ${CENTRE})`}
        className={running ? "stroke-accent" : "stroke-white/35"}
      />
    </svg>
  );
}
