import { useState } from "react";
import {
  CardBody,
  KindBadge,
  SegmentList,
  Stat,
} from "../components/ActivityBits";
import { BentoCard, BentoGrid, PageHeader } from "../components/Bento";
import { HourRing } from "../components/HourRing";
import { useNotImplemented } from "../components/NotImplemented";
import { useActivity } from "../hooks/useActivity";
import { formatClock, segmentDuration } from "../lib/activity";
import { formatDuration } from "../lib/timers";

type Scale = "day" | "week" | "month" | "year";

const scaleLabels: Record<Scale, string> = {
  day: "Day",
  week: "Week",
  month: "Month",
  year: "Year",
};

export function Sessions() {
  const activity = useActivity();
  const { show } = useNotImplemented();
  const snapshot = activity.snapshot;
  const segments = snapshot?.segments ?? [];
  const now = activity.now;

  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [scale, setScale] = useState<Scale>("day");

  const selected =
    segments.find((segment) => segment.id === selectedId) ?? null;
  const current = snapshot?.current ?? null;
  // Only a Focus/Break is a "session" the user controls; plain activity is the
  // automatic capture doing its job.
  const sessionOpen =
    current !== null && (current.kind === "focus" || current.kind === "break");

  const chooseScale = (next: Scale): void => {
    if (next === "day") {
      setScale(next);
      return;
    }
    show(`${scaleLabels[next]} view`);
  };

  return (
    <main className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-5.5">
      <PageHeader
        title="Sessions"
        description="Timeline & timer · automatic tracking core, separate from manual Trackers"
        status="live"
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
          title="Timeline"
          description="Every auto-captured session today, chronological. Select one to review it."
          status="live"
        >
          <div className="flex gap-1">
            {(Object.keys(scaleLabels) as Scale[]).map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => chooseScale(option)}
                className={`rounded-md px-2 py-1 font-mono text-[10.5px] uppercase tracking-wider ${
                  scale === option
                    ? "bg-accent-soft text-accent"
                    : "text-white/40 hover:bg-white/5"
                }`}
              >
                {scaleLabels[option]}
              </button>
            ))}
          </div>
          <CardBody>
            <SegmentList
              segments={segments}
              now={now}
              selectedId={selectedId}
              onSelect={(segment) => setSelectedId(segment.id)}
            />
          </CardBody>
        </BentoCard>

        <BentoCard
          span="md"
          title="Session timer"
          description="The live session, or start a Focus / Break by hand."
          status="live"
        >
          <div className="flex items-center gap-4">
            {sessionOpen && current !== null ? (
              <>
                <HourRing
                  elapsed={segmentDuration(current, now)}
                  running
                  onToggle={activity.stopSession}
                />
                <div className="min-w-0">
                  <KindBadge kind={current.kind} />
                  <div className="mt-1 truncate text-[13px] font-semibold text-white">
                    {current.title}
                  </div>
                  <div className="font-mono text-[20px] tabular-nums text-accent">
                    {formatDuration(segmentDuration(current, now))}
                  </div>
                  <button
                    type="button"
                    onClick={activity.stopSession}
                    className="mt-1 rounded-md border border-white/10 bg-white/5 px-2 py-1 text-[11px] text-white/70 hover:bg-white/10"
                  >
                    Stop session
                  </button>
                </div>
              </>
            ) : (
              <div className="min-w-0">
                <KindBadge kind="activity" />
                <div className="mt-1 truncate text-[13px] text-white/70">
                  {current === null
                    ? "No session running"
                    : current.title.length > 0
                      ? current.title
                      : current.app}
                </div>
                <div className="font-mono text-[11px] text-white/40">
                  Tracking automatically — start Focus or Break to take over.
                </div>
              </div>
            )}
          </div>
          <div className="mt-auto flex gap-2">
            <button
              type="button"
              onClick={() => activity.startSession("focus", "Deep work")}
              className="rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3 py-1.5 text-[12px] font-semibold text-[#04160c]"
            >
              Start Focus
            </button>
            <button
              type="button"
              onClick={() => activity.startSession("break", "Break")}
              className="rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12px] text-white/75 hover:bg-white/10"
            >
              Start Break
            </button>
          </div>
        </BentoCard>

        <BentoCard
          span="md"
          title="Automatic capture"
          description="Background logging of the active app and window title, no manual start."
          status="live"
        >
          <div className="flex items-center gap-2">
            <span
              className={`size-2 rounded-full ${
                snapshot?.captureEnabled ? "bg-accent" : "bg-white/30"
              }`}
            />
            <span className="text-[12.5px] text-white/75">
              {snapshot?.captureEnabled ? "Capture running" : "Capture paused"}
            </span>
            <button
              type="button"
              onClick={() =>
                activity.setCaptureEnabled(!(snapshot?.captureEnabled ?? false))
              }
              className="ml-auto rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12px] text-white/75 hover:bg-white/10"
            >
              {snapshot?.captureEnabled ? "Pause" : "Resume"}
            </button>
          </div>
          <div className="min-w-0 rounded-lg border border-white/5 bg-black/20 px-2.5 py-2">
            <div className="truncate text-[12.5px] text-white/80">
              {current?.title.length ? current.title : (current?.app ?? "—")}
            </div>
            <div className="font-mono text-[10.5px] text-white/35">
              current window
            </div>
          </div>
          <p className="text-[10.5px] leading-snug text-white/35">
            macOS captures app names out of the box; window titles need Screen
            Recording permission for OpenRize.
          </p>
        </BentoCard>

        <BentoCard
          span="md"
          title="Review panel"
          description="Inspect a captured entry and confirm it."
          status="live"
        >
          {selected === null ? (
            <p className="rounded-lg border border-dashed border-white/10 px-3 py-6 text-center text-[12px] text-white/35">
              Select a block on the timeline.
            </p>
          ) : (
            <div className="flex min-h-0 flex-col gap-2">
              <div className="flex items-center justify-between gap-2">
                <KindBadge kind={selected.kind} />
                <span className="font-mono text-[11px] tabular-nums text-white/45">
                  {formatClock(selected.startedAt)}–
                  {selected.endedAt === null
                    ? "now"
                    : formatClock(selected.endedAt)}
                </span>
              </div>
              <div className="min-w-0">
                <div className="truncate text-[13px] font-semibold text-white">
                  {selected.title.length > 0 ? selected.title : selected.app}
                </div>
                <div className="truncate text-[11px] text-white/45">
                  {selected.app}
                </div>
              </div>
              <button
                type="button"
                disabled={selected.reviewed}
                onClick={() => activity.markReviewed(selected.id)}
                className="self-start rounded-lg border border-white/10 bg-white/5 px-3 py-1.5 text-[12px] text-white/75 hover:bg-white/10 disabled:opacity-40"
              >
                {selected.reviewed ? "Reviewed" : "Mark reviewed"}
              </button>
              <div className="mt-auto flex gap-2">
                <button
                  type="button"
                  onClick={() => show("AI category suggestions")}
                  className="rounded-lg border border-white/10 bg-white/5 px-2.5 py-1.5 text-[11.5px] text-white/65 hover:bg-white/10"
                >
                  Suggest category
                </button>
                <button
                  type="button"
                  onClick={() => show("Create task")}
                  className="rounded-lg border border-white/10 bg-white/5 px-2.5 py-1.5 text-[11.5px] text-white/65 hover:bg-white/10"
                >
                  Create task
                </button>
              </div>
            </div>
          )}
        </BentoCard>

        <BentoCard
          span="sm"
          title="Idle detection"
          description="Stops logging and starts a Break after this much inactivity."
          status="live"
        >
          <div className="flex items-end gap-2">
            <label className="flex flex-col gap-1">
              <span className="font-mono text-[10px] uppercase tracking-wider text-white/35">
                Minutes
              </span>
              <input
                type="number"
                min={1}
                key={snapshot?.idleThresholdMs ?? 0}
                defaultValue={Math.round(
                  (snapshot?.idleThresholdMs ?? 0) / 60_000,
                )}
                onBlur={(event) => {
                  const minutes = Number(event.target.value);
                  if (Number.isFinite(minutes) && minutes >= 1) {
                    activity.setIdleThreshold(Math.round(minutes));
                  }
                }}
                onKeyDown={(event) => {
                  if (event.key === "Enter") event.currentTarget.blur();
                }}
                className="w-16 rounded-md border border-white/10 bg-black/40 px-2 py-1 text-[12.5px] text-white outline-none focus:border-accent/30"
              />
            </label>
            <Stat
              label="Idle now"
              value={formatDuration(snapshot?.idleMs ?? 0)}
            />
          </div>
        </BentoCard>
      </BentoGrid>
    </main>
  );
}
