import {
  CardBody,
  KindBadge,
  SegmentList,
  Stat,
} from "../components/ActivityBits";
import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";
import { useNotImplemented } from "../components/NotImplemented";
import { useActivity } from "../hooks/useActivity";
import { segmentDuration } from "../lib/activity";
import { formatDuration } from "../lib/timers";

/** A gentle daily floor, not a clinical target: four breaks and half an hour. */
const BREAK_TARGET_COUNT = 4;
const BREAK_TARGET_MS = 30 * 60 * 1000;

export function Breaks() {
  const activity = useActivity();
  const { show } = useNotImplemented();
  const snapshot = activity.snapshot;
  const now = activity.now;

  const breakSegments = (snapshot?.segments ?? []).filter(
    (segment) => segment.kind === "break",
  );
  const longest = breakSegments.reduce(
    (max, segment) => Math.max(max, segmentDuration(segment, now)),
    0,
  );
  const totalMs = snapshot?.breakMs ?? 0;

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Breaks"
        description="Rest-focused session type"
        status="in-progress"
      />

      {activity.error !== null && (
        <p
          role="alert"
          className="rounded-[10px] border border-red-400/40 bg-red-400/10 px-3.5 py-2.5 text-[13px] text-red-200"
        >
          {activity.error}
        </p>
      )}

      <BentoGrid>
        <BentoCard
          span="lg"
          title="Break metrics"
          description="How much rest today's session log actually contains."
          status="live"
        >
          <div className="flex gap-6">
            <Stat
              label="Breaks"
              value={String(breakSegments.length)}
              hint={`target ${BREAK_TARGET_COUNT}`}
            />
            <Stat
              label="Break time"
              value={formatDuration(totalMs)}
              hint={`target ${formatDuration(BREAK_TARGET_MS)}`}
            />
            <Stat label="Longest" value={formatDuration(longest)} />
          </div>
          <div className="flex flex-col gap-1.5">
            <div className="h-1.5 overflow-hidden rounded-full bg-white/5">
              <div
                className="h-full rounded-full bg-sky-400/70"
                style={{
                  width: `${Math.min(
                    100,
                    BREAK_TARGET_MS === 0
                      ? 0
                      : (totalMs / BREAK_TARGET_MS) * 100,
                  )}%`,
                }}
              />
            </div>
            <p className="font-mono text-[10.5px] text-white/35">
              {Math.round(totalMs / 60_000)} of{" "}
              {Math.round(BREAK_TARGET_MS / 60_000)} target minutes today
            </p>
          </div>
          <CardBody>
            <SegmentList
              segments={breakSegments}
              now={now}
              emptyHint="No breaks taken yet today."
            />
          </CardBody>
        </BentoCard>

        <BentoCard
          span="md"
          title="Idle-triggered breaks"
          description="When the OS reports no input for the threshold, capture opens a Break for you."
          status="live"
        >
          <div className="flex items-center gap-2">
            <span className="size-2 rounded-full bg-sky-400" />
            <span className="text-[12.5px] text-white/75">
              Fires after{" "}
              {Math.round((snapshot?.idleThresholdMs ?? 0) / 60_000)} min idle
            </span>
          </div>
          <Stat
            label="Idle now"
            value={formatDuration(snapshot?.idleMs ?? 0)}
            hint="Change the threshold on the Sessions page"
          />
        </BentoCard>

        <BentoCard
          span="sm"
          title="Manual break"
          description="Take a break on demand."
          status="live"
        >
          <button
            type="button"
            onClick={() => activity.startSession("break", "Break")}
            className="self-start rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3 py-1.5 text-[12px] font-semibold text-[#04160c]"
          >
            Start Break
          </button>
          {snapshot?.current?.kind === "break" && (
            <div className="flex items-center gap-2">
              <KindBadge kind="break" />
              <span className="text-[12px] text-white/70">Running</span>
            </div>
          )}
        </BentoCard>

        <BentoCard
          span="md"
          title="Guided meditation"
          description="In-app guided meditations to use during a break."
        >
          <button
            type="button"
            onClick={() => show("Guided meditation")}
            className="self-start rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12px] text-white/70 hover:bg-white/10"
          >
            Browse sessions
          </button>
        </BentoCard>
      </BentoGrid>
    </main>
  );
}
