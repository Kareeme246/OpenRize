import { useMemo } from "react";
import { EmptyState } from "../../components/Page";
import { Tooltip } from "../../components/Tooltip";
import type { DayThreads, Rail, Thread } from "../../lib/agents";
import { formatDuration, formatTime } from "../../lib/format";
import type { Project } from "../../lib/types";
import { dayLength, gutterLabel, hourOffset, timelineFor } from "./timeline";

export type ThreadsLayout = "lanes" | "timeline";

const NO_PROJECT = "var(--fg-faint)";
const HOUR_MS = 3_600_000;

interface ThreadsViewProps {
  day: DayThreads | undefined;
  dayStart: number;
  layout: ThreadsLayout;
  projectById: Map<string, Project>;
  selectedId?: string;
  loading: boolean;
  onSelect: (id: string) => void;
  now: number;
}

function colorOf(
  thread: Thread,
  projectById: Map<string, Project>,
): string | undefined {
  return thread.projectId
    ? projectById.get(thread.projectId)?.color
    : undefined;
}

function nameOf(thread: Thread, projectById: Map<string, Project>): string {
  if (!thread.projectId) return "No project";
  return projectById.get(thread.projectId)?.name ?? "Unknown project";
}

/** Hatching for agent time, so it reads as drawn-not-counted. */
function hatch(color: string, waiting: boolean): React.CSSProperties {
  const tone = waiting ? "var(--review)" : color;
  return {
    backgroundImage: `repeating-linear-gradient(135deg, ${tone} 0 2px, transparent 2px 6px)`,
    border: `1px ${waiting ? "dashed" : "solid"} color-mix(in srgb, ${tone} 60%, transparent)`,
  };
}

/** Your time on a thread, and the agents' beside it as a ghost. */
function ThreadTotals({ thread }: { thread: Thread }) {
  return (
    <div className="text-[10.5px] text-fg-faint tabular-nums leading-tight">
      <div>
        {thread.youMs > 0
          ? `you ${formatDuration(thread.youMs)}`
          : "no time of yours"}
      </div>
      {thread.agentsMs > 0 && (
        <div className="opacity-70">
          +{formatDuration(thread.agentsMs)} agents
        </div>
      )}
    </div>
  );
}

function railLabel(rail: Rail, name: string): string {
  const what = rail.state === "needsYou" ? "waited on you" : "worked";
  return `${rail.agent} ${what} · ${name} · ${formatTime(rail.startedAt)}-${formatTime(rail.endedAt)} · ${formatDuration(rail.endedAt - rail.startedAt)}`;
}

/** Switches and the longest stretch, plus what agents ran (never in the total). */
export function FocusStrip({
  day,
  projectById,
}: {
  day: DayThreads;
  projectById: Map<string, Project>;
}) {
  const longest = day.focus.longestProjectId
    ? projectById.get(day.focus.longestProjectId)?.name
    : undefined;
  return (
    <div className="flex flex-wrap items-baseline gap-x-5 gap-y-1 border-line border-b px-4 py-2 text-[11.5px] text-fg-muted">
      <span>
        <b className="font-semibold text-fg-strong tabular-nums">
          {formatDuration(day.workMs)}
        </b>{" "}
        you
      </span>
      <span>
        <b className="font-semibold text-fg-strong tabular-nums">
          {day.focus.switches}
        </b>{" "}
        {day.focus.switches === 1 ? "switch" : "switches"}
      </span>
      <span>
        longest stretch{" "}
        <b className="font-semibold text-fg-strong tabular-nums">
          {formatDuration(day.focus.longestMs)}
        </b>
        {longest ? ` on ${longest}` : ""}
      </span>
      {day.agentsMs > 0 && (
        <span className="text-fg-faint">
          Agents ran {formatDuration(day.agentsMs)} (not in total)
        </span>
      )}
    </div>
  );
}

export function ThreadsView(props: ThreadsViewProps) {
  const { day, loading, projectById } = props;
  if (!day || day.threads.length === 0) {
    return (
      <div className="min-h-0 flex-1 overflow-y-auto">
        {!loading && (
          <EmptyState
            title="No threads for this day"
            hint="Each project you work on gets its own thread as soon as there is time on it."
          />
        )}
      </div>
    );
  }
  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      <FocusStrip day={day} projectById={projectById} />
      {props.layout === "lanes" ? (
        <ThreadLanes {...props} day={day} />
      ) : (
        <ThreadTimeline {...props} day={day} />
      )}
    </div>
  );
}

function useExtent(day: DayThreads, dayStart: number, hourHeight: number) {
  return useMemo(() => {
    const spans: { start: number; end: number; dayStart: number }[] = [];
    for (const thread of day.threads) {
      for (const visit of thread.visits) {
        spans.push({ start: visit.startedAt, end: visit.endedAt, dayStart });
      }
      for (const rail of thread.rails) {
        spans.push({ start: rail.startedAt, end: rail.endedAt, dayStart });
      }
    }
    return timelineFor(spans, "elapsed", hourHeight, dayLength(dayStart));
  }, [day, dayStart, hourHeight]);
}

const LANE_HOUR = 64;

/** One vertical lane per thread: solid visits, hatched rails, a faint band. */
function ThreadLanes({
  day,
  dayStart,
  projectById,
  selectedId,
  onSelect,
  now,
}: ThreadsViewProps & { day: DayThreads }) {
  const timeline = useExtent(day, dayStart, LANE_HOUR);
  const top = (ms: number): number =>
    (hourOffset(ms, dayStart, "elapsed") - timeline.startHour) * LANE_HOUR;

  return (
    <div className="min-h-0 flex-1 overflow-auto">
      <div className="sticky top-0 z-20 flex border-line border-b bg-panel pl-14">
        {day.threads.map((thread) => (
          <div
            key={thread.projectId ?? "none"}
            className="min-w-[132px] flex-1 border-line border-l px-2 py-1.5"
          >
            <div
              className="truncate font-semibold text-[11.5px]"
              style={{ color: colorOf(thread, projectById) ?? NO_PROJECT }}
            >
              {nameOf(thread, projectById)}
            </div>
            <ThreadTotals thread={thread} />
          </div>
        ))}
      </div>
      <div className="relative mt-3 flex" style={{ height: timeline.height }}>
        <div className="relative w-14 shrink-0 select-none text-right font-medium text-[11px] text-fg-faint">
          {timeline.hours.map((hour) => (
            <div
              key={hour}
              className="absolute right-2 -translate-y-2 whitespace-nowrap"
              style={{ top: (hour - timeline.startHour) * LANE_HOUR }}
            >
              {gutterLabel(hour, dayStart, "elapsed")}
            </div>
          ))}
        </div>
        <div className="relative flex min-w-0 flex-1">
          {timeline.hours.map((hour) => (
            <div
              key={`line-${hour}`}
              className="pointer-events-none absolute right-0 left-0 border-line-soft border-b"
              style={{ top: (hour - timeline.startHour) * LANE_HOUR }}
            />
          ))}
          {now >= dayStart &&
            now < dayStart + dayLength(dayStart) * HOUR_MS && (
              <div
                className="pointer-events-none absolute right-0 left-0 z-30 h-px bg-danger/70"
                style={{ top: top(now) }}
              />
            )}
          {day.threads.map((thread) => {
            const color = colorOf(thread, projectById) ?? NO_PROJECT;
            const name = nameOf(thread, projectById);
            return (
              <div
                key={thread.projectId ?? "none"}
                className="relative min-w-[132px] flex-1 border-line border-l"
              >
                {thread.bands.map((band) => (
                  <div
                    key={`${band.jobId}-${band.startedAt}`}
                    className="pointer-events-none absolute right-0 left-0 opacity-[0.07]"
                    style={{
                      top: top(band.startedAt),
                      height: Math.max(
                        2,
                        top(band.endedAt) - top(band.startedAt),
                      ),
                      backgroundColor: color,
                    }}
                  />
                ))}
                {thread.visits.map((visit) => {
                  const height = Math.max(
                    3,
                    top(visit.endedAt) - top(visit.startedAt) - 2,
                  );
                  return (
                    <Tooltip
                      key={`${visit.entryId}-${visit.startedAt}`}
                      content={`${name} · ${formatTime(visit.startedAt)}-${formatTime(visit.endedAt)} · ${formatDuration(visit.endedAt - visit.startedAt)}`}
                    >
                      <button
                        type="button"
                        onClick={() => onSelect(visit.entryId)}
                        aria-label={`${name}, ${formatDuration(visit.endedAt - visit.startedAt)}`}
                        className={`calendar-entry absolute right-5 left-1 overflow-hidden rounded-md px-1.5 text-left font-medium text-[10.5px] leading-tight ${
                          visit.entryId === selectedId
                            ? "z-20 ring-2 ring-accent"
                            : "z-10"
                        }`}
                        style={{
                          top: top(visit.startedAt),
                          height,
                          backgroundColor: `color-mix(in srgb, ${color} 28%, var(--bg-panel))`,
                          color,
                          borderLeft: `3px solid ${color}`,
                        }}
                      >
                        {height > 18
                          ? formatDuration(visit.endedAt - visit.startedAt)
                          : ""}
                      </button>
                    </Tooltip>
                  );
                })}
                {thread.rails.map((rail) => (
                  <Tooltip
                    key={`${rail.jobId}-${rail.startedAt}-${rail.state}`}
                    content={railLabel(rail, name)}
                  >
                    <div
                      className="absolute right-1 z-10 w-3 rounded-sm"
                      style={{
                        top: top(rail.startedAt),
                        height: Math.max(
                          3,
                          top(rail.endedAt) - top(rail.startedAt) - 1,
                        ),
                        ...hatch(color, rail.state === "needsYou"),
                      }}
                      role="img"
                      aria-label={railLabel(rail, name)}
                    />
                  </Tooltip>
                ))}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

const ROW_HEIGHT = 44;
const TIMELINE_HOUR = 72;
/** Room before the first hour so its label is not clipped. */
const TIMELINE_PAD = 20;

/** The same threads as rows across the day: label left, time to the right. */
function ThreadTimeline({
  day,
  dayStart,
  projectById,
  selectedId,
  onSelect,
  now,
}: ThreadsViewProps & { day: DayThreads }) {
  const timeline = useExtent(day, dayStart, TIMELINE_HOUR);
  const left = (ms: number): number =>
    (hourOffset(ms, dayStart, "elapsed") - timeline.startHour) * TIMELINE_HOUR +
    TIMELINE_PAD;
  const width = timeline.height + TIMELINE_PAD;

  return (
    <div className="min-h-0 flex-1 overflow-auto">
      <div className="flex" style={{ minWidth: width + 160 }}>
        <div className="sticky left-0 z-30 w-40 shrink-0 border-line border-r bg-panel">
          <div className="h-7 border-line border-b" />
          {day.threads.map((thread) => (
            <div
              key={thread.projectId ?? "none"}
              className="flex flex-col justify-center border-line-soft border-b px-3"
              style={{ height: ROW_HEIGHT }}
            >
              <div
                className="truncate font-semibold text-[11.5px]"
                style={{ color: colorOf(thread, projectById) ?? NO_PROJECT }}
              >
                {nameOf(thread, projectById)}
              </div>
              <ThreadTotals thread={thread} />
            </div>
          ))}
        </div>
        <div className="relative min-w-0 flex-1" style={{ width }}>
          <div className="relative h-7 border-line border-b select-none">
            {timeline.hours.map((hour) => (
              <span
                key={hour}
                className="absolute top-1.5 -translate-x-1/2 font-medium text-[10.5px] text-fg-faint"
                style={{
                  left:
                    (hour - timeline.startHour) * TIMELINE_HOUR + TIMELINE_PAD,
                }}
              >
                {gutterLabel(hour, dayStart, "elapsed")}
              </span>
            ))}
          </div>
          {timeline.hours.map((hour) => (
            <div
              key={`tick-${hour}`}
              className="pointer-events-none absolute top-7 bottom-0 border-line-soft border-l"
              style={{
                left:
                  (hour - timeline.startHour) * TIMELINE_HOUR + TIMELINE_PAD,
              }}
            />
          ))}
          {now >= dayStart &&
            now < dayStart + dayLength(dayStart) * HOUR_MS && (
              <div
                className="pointer-events-none absolute top-7 bottom-0 z-30 w-px bg-danger/70"
                style={{ left: left(now) }}
              />
            )}
          {day.threads.map((thread) => {
            const color = colorOf(thread, projectById) ?? NO_PROJECT;
            const name = nameOf(thread, projectById);
            return (
              <div
                key={thread.projectId ?? "none"}
                className="relative border-line-soft border-b"
                style={{ height: ROW_HEIGHT }}
              >
                {thread.bands.map((band) => (
                  <div
                    key={`${band.jobId}-${band.startedAt}`}
                    className="pointer-events-none absolute inset-y-1 opacity-[0.08]"
                    style={{
                      left: left(band.startedAt),
                      width: Math.max(
                        2,
                        left(band.endedAt) - left(band.startedAt),
                      ),
                      backgroundColor: color,
                    }}
                  />
                ))}
                {thread.rails.map((rail) => (
                  <Tooltip
                    key={`${rail.jobId}-${rail.startedAt}-${rail.state}`}
                    content={railLabel(rail, name)}
                  >
                    <div
                      className="absolute bottom-1 z-10 h-2.5 rounded-sm"
                      style={{
                        left: left(rail.startedAt),
                        width: Math.max(
                          3,
                          left(rail.endedAt) - left(rail.startedAt) - 1,
                        ),
                        ...hatch(color, rail.state === "needsYou"),
                      }}
                      role="img"
                      aria-label={railLabel(rail, name)}
                    />
                  </Tooltip>
                ))}
                {thread.visits.map((visit) => {
                  const w = Math.max(
                    3,
                    left(visit.endedAt) - left(visit.startedAt) - 2,
                  );
                  return (
                    <Tooltip
                      key={`${visit.entryId}-${visit.startedAt}`}
                      content={`${name} · ${formatTime(visit.startedAt)}-${formatTime(visit.endedAt)} · ${formatDuration(visit.endedAt - visit.startedAt)}`}
                    >
                      <button
                        type="button"
                        onClick={() => onSelect(visit.entryId)}
                        aria-label={`${name}, ${formatDuration(visit.endedAt - visit.startedAt)}`}
                        className={`calendar-entry absolute top-1.5 h-[18px] overflow-hidden rounded-md px-1 text-left font-medium text-[10px] leading-[18px] ${
                          visit.entryId === selectedId
                            ? "z-20 ring-2 ring-accent"
                            : "z-10"
                        }`}
                        style={{
                          left: left(visit.startedAt),
                          width: w,
                          backgroundColor: `color-mix(in srgb, ${color} 30%, var(--bg-panel))`,
                          color,
                          borderLeft: `3px solid ${color}`,
                        }}
                      >
                        {w > 44
                          ? formatDuration(visit.endedAt - visit.startedAt)
                          : ""}
                      </button>
                    </Tooltip>
                  );
                })}
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
