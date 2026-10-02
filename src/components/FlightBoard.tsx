import { useMemo } from "react";
import type { Board, JobView, LiveAgent, PaneState } from "../lib/agents";
import * as api from "../lib/api";
import { formatDuration } from "../lib/format";
import type { Project } from "../lib/types";

const MAX_ROWS = 4;

const STATE_ORDER: Record<PaneState, number> = {
  needsYou: 0,
  ready: 1,
  running: 2,
  quiet: 3,
  idle: 4,
  unknown: 5,
};

const STATE_TEXT: Record<PaneState, string> = {
  needsYou: "needs you",
  ready: "ready",
  running: "working",
  quiet: "quiet",
  idle: "idle",
  unknown: "unknown",
};

function dotClass(state: PaneState): string {
  if (state === "running") return "bg-accent animate-pulse";
  if (state === "needsYou") return "bg-review animate-pulse";
  if (state === "ready") return "bg-review";
  return "bg-fg-ghost";
}

/** A finished turn nobody supervised: it counts only once the person says so. */
function isUnwatched(job: JobView): boolean {
  return (
    job.pendingMs > 0 &&
    !job.confirmed &&
    job.phase !== "running" &&
    job.phase !== "needsYou"
  );
}

/** `Waited on you 46m` and friends, as one small stat. */
function Stat({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone?: "review" | "accent";
}) {
  const color =
    tone === "review"
      ? "text-review"
      : tone === "accent"
        ? "text-accent"
        : "text-fg-strong";
  return (
    <div className="min-w-0">
      <div className="truncate font-semibold text-[10px] text-fg-faint uppercase tracking-wider">
        {label}
      </div>
      <div
        className={`font-bold text-[15px] tabular-nums leading-tight ${color}`}
      >
        {value}
      </div>
    </div>
  );
}

function Row({
  agent,
  projectById,
  now,
}: {
  agent: LiveAgent;
  projectById: Map<string, Project>;
  now: number;
}) {
  const project = agent.projectId
    ? projectById.get(agent.projectId)
    : undefined;
  return (
    <li className="flex min-w-0 items-center gap-2 py-1">
      <span
        className={`size-2 shrink-0 rounded-full ${dotClass(agent.state)}`}
      />
      <span className="min-w-0 flex-1 truncate">
        <span className="font-medium text-fg">
          {project?.name ?? "No project"}
        </span>
        <span className="text-fg-faint"> · {agent.agent}</span>
      </span>
      <span
        className={`shrink-0 text-[11px] ${
          agent.state === "needsYou" || agent.state === "ready"
            ? "font-semibold text-review"
            : "text-fg-soft"
        }`}
      >
        {STATE_TEXT[agent.state]} ·{" "}
        {formatDuration(Math.max(0, now - agent.since))}
      </span>
    </li>
  );
}

/**
 * The Pulse flight board: what you are in right now, what runs, what waits
 * on you, and what you made agents wait today. Metadata only; nothing here
 * is a terminal's content.
 */
export function FlightBoard({
  board,
  projectById,
  now,
}: {
  board: Board;
  projectById: Map<string, Project>;
  now: number;
}) {
  const rows = useMemo(
    () =>
      [...board.live].sort(
        (a, b) =>
          STATE_ORDER[a.state] - STATE_ORDER[b.state] || a.since - b.since,
      ),
    [board.live],
  );
  const focused = board.live.find((agent) => agent.focused);
  const focusedProject = focused?.projectId
    ? projectById.get(focused.projectId)
    : undefined;
  const pending = board.jobs.filter(isUnwatched);

  const countAll = (): void => {
    for (const job of pending) void api.confirmAgentJob(job.id);
  };

  return (
    <>
      <div className="mb-2 flex items-baseline justify-between font-semibold text-[10.5px] text-fg-faint uppercase tracking-wider">
        <span>Agents</span>
        {focused && (
          <span className="font-normal normal-case tracking-normal text-fg-soft">
            Now in {focused.agent}
            {focusedProject ? ` on ${focusedProject.name}` : ""}
          </span>
        )}
      </div>
      <div className="grid grid-cols-3 gap-3">
        <Stat label="Running" value={String(board.running)} tone="accent" />
        <Stat
          label="Needs you"
          value={String(board.needsYou + board.ready)}
          tone={board.needsYou + board.ready > 0 ? "review" : undefined}
        />
        <Stat label="Waited today" value={formatDuration(board.waitedMs)} />
      </div>
      {rows.length > 0 && (
        <ul className="mt-2">
          {rows.slice(0, MAX_ROWS).map((agent) => (
            <Row
              key={agent.key}
              agent={agent}
              projectById={projectById}
              now={now}
            />
          ))}
          {rows.length > MAX_ROWS && (
            <li className="pt-0.5 text-[10.5px] text-fg-faint">
              +{rows.length - MAX_ROWS} more
            </li>
          )}
        </ul>
      )}
      {pending.length > 0 && (
        <div className="mt-2 flex items-center gap-2 rounded-lg bg-review/10 px-2.5 py-1.5">
          <span className="min-w-0 flex-1 truncate text-[11px] text-fg-muted">
            {pending.length === 1 ? "1 turn" : `${pending.length} turns`} you
            did not watch
          </span>
          <button
            type="button"
            onClick={countAll}
            className="shrink-0 rounded-md bg-review/15 px-2 py-1 font-semibold text-[11px] text-review transition-colors hover:bg-review/25"
          >
            Count {pending.length === 1 ? "it" : "them"}
          </button>
        </div>
      )}
    </>
  );
}

/** Whether the board has anything to show: otherwise Pulse stays as it was. */
export function boardHasContent(board: Board): boolean {
  return (
    board.live.length > 0 ||
    board.jobs.length > 0 ||
    board.waitedMs > 0 ||
    board.toConfirm > 0
  );
}
