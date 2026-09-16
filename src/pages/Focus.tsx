import { CardBody, KindBadge, SegmentList, Stat } from "../components/ActivityBits";
import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";
import { useNotImplemented } from "../components/NotImplemented";
import { useActivity } from "../hooks/useActivity";
import { segmentDuration } from "../lib/activity";
import { formatDuration } from "../lib/timers";

export function Focus() {
  const activity = useActivity();
  const { show } = useNotImplemented();
  const snapshot = activity.snapshot;
  const now = activity.now;

  const focusSegments = (snapshot?.segments ?? []).filter(
    (segment) => segment.kind === "focus",
  );
  const longest = focusSegments.reduce(
    (max, segment) => Math.max(max, segmentDuration(segment, now)),
    0,
  );
  // Context switches are the one honest quality signal already captured: every
  // separate window segment is a switch. The full 20+ attribute score is not
  // built, so this card is Partial, not Live.
  const switches = (snapshot?.segments ?? []).filter(
    (segment) => segment.kind === "activity",
  ).length;

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Focus"
        description="Deep-work session type"
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
          title="Focus quality"
          description="Time spent in Focus today, plus the context-switch signal we already capture."
          status="in-progress"
        >
          <div className="flex gap-6">
            <Stat
              label="Focus time"
              value={formatDuration(snapshot?.focusMs ?? 0)}
            />
            <Stat label="Sessions" value={String(focusSegments.length)} />
            <Stat label="Longest" value={formatDuration(longest)} />
          </div>
          <div className="flex flex-col gap-1 rounded-lg border border-white/5 bg-black/20 px-2.5 py-2">
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-[12px] text-white/70">Window switches</span>
              <span className="font-mono text-[13px] tabular-nums text-white">
                {switches}
              </span>
            </div>
            <p className="text-[10.5px] leading-snug text-white/35">
              The full 20+ attribute quality score needs tracking rules, so only
              this switch count is real today.
            </p>
          </div>
          <CardBody>
            <SegmentList
              segments={focusSegments}
              now={now}
              emptyHint="No Focus sessions today."
            />
          </CardBody>
        </BentoCard>

        <BentoCard
          span="sm"
          title="Manual start"
          description="Begin a Focus session right now."
          status="live"
        >
          <button
            type="button"
            onClick={() => activity.startSession("focus", "Deep work")}
            className="self-start rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3 py-1.5 text-[12px] font-semibold text-[#04160c]"
          >
            Start Focus
          </button>
          {snapshot?.current?.kind === "focus" && (
            <div className="flex items-center gap-2">
              <KindBadge kind="focus" />
              <span className="text-[12px] text-white/70">Running</span>
            </div>
          )}
        </BentoCard>

        <BentoCard
          span="md"
          title="Automatic trigger"
          description="Starts a Focus once ~75% of a 15+ minute window is spent in apps rules mark as Focus."
        >
          <button
            type="button"
            onClick={() => show("Automatic Focus trigger")}
            className="self-start rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12px] text-white/70 hover:bg-white/10"
          >
            Configure rules
          </button>
        </BentoCard>

        <BentoCard
          span="md"
          title="Focus rules"
          description="Choose which apps and websites count toward Focus mode."
        >
          <button
            type="button"
            onClick={() => show("Focus rules")}
            className="self-start rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12px] text-white/70 hover:bg-white/10"
          >
            Edit rules
          </button>
        </BentoCard>

        <BentoCard
          span="sm"
          title="Calendar keyword trigger"
          description="Auto-starts from calendar events matching a keyword, e.g. #rize-focus."
        >
          <button
            type="button"
            onClick={() => show("Calendar keyword trigger")}
            className="self-start rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12px] text-white/70 hover:bg-white/10"
          >
            Set keywords
          </button>
        </BentoCard>
      </BentoGrid>
    </main>
  );
}
