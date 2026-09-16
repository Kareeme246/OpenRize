const SIZE = 68;
const STROKE = 5;
const RADIUS = (SIZE - STROKE) / 2;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
const CENTRE = SIZE / 2;

interface HourRingProps {
  now: number;
  running: boolean;
}

/** How far the wall clock has moved through the current hour, 0–1. */
export function hourProgress(now: number): number {
  const date = new Date(now);
  return (date.getMinutes() * 60 + date.getSeconds()) / 3600;
}

/** A clock-style dial: the arc sweeps once per hour, whether or not the
 *  tracker is running — it reads the wall clock, not the timer. */
export function HourRing({ now, running }: HourRingProps) {
  const progress = hourProgress(now);
  const minutes = new Date(now).getMinutes();

  return (
    <svg
      viewBox={`0 0 ${SIZE} ${SIZE}`}
      className="size-17 shrink-0"
      role="img"
      aria-label={`${minutes} minutes into the current hour`}
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
