import { useState } from "react";
import {
  EmptyState,
  InlineError,
  PageHeader,
  SkeletonRows,
} from "../components/Page";
import { StopwatchCard } from "../components/StopwatchCard";
import { type TimersApi, useTimers } from "../hooks/useTimers";
import { elapsedMs, formatDuration } from "../lib/timers";

interface TimersProps {
  api?: TimersApi;
}

export function Timers({ api: propApi }: TimersProps = {}) {
  const defaultApi = useTimers();
  const api = propApi ?? defaultApi;
  const [label, setLabel] = useState("");
  const trimmed = label.trim();

  const submit = (): void => {
    if (trimmed.length === 0) return;
    api.create(trimmed);
    setLabel("");
  };

  const runningCount = api.timers.filter(
    (timer) => timer.startedAt !== null,
  ).length;
  const totalMs = api.timers.reduce(
    (sum, timer) => sum + elapsedMs(timer, api.now),
    0,
  );

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-canvas text-fg">
      <PageHeader title="Timers">
        <div className="flex items-center gap-4 font-mono text-[12px]">
          <div className="flex items-center gap-1.5">
            <span className="font-semibold text-accent">{runningCount}</span>
            <span className="text-fg-faint">running</span>
          </div>
          <div className="flex items-center gap-1.5">
            <span className="font-semibold text-fg-strong">
              {formatDuration(totalMs)}
            </span>
            <span className="text-fg-faint">total</span>
          </div>
        </div>
      </PageHeader>

      <main className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
        <form
          onSubmit={(event) => {
            event.preventDefault();
            submit();
          }}
          className="flex gap-2.5"
        >
          <input
            value={label}
            placeholder="What are you working on?"
            aria-label="New tracker name"
            onChange={(event) => setLabel(event.target.value)}
            className="min-w-0 flex-1 rounded-lg border border-line bg-surface px-3.5 py-2 text-[13px] text-fg placeholder:text-fg-faint outline-none focus:border-accent"
          />
          <button
            type="submit"
            disabled={trimmed.length === 0}
            className="shrink-0 rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-4 py-2 text-[13px] font-semibold text-accent-fg transition-opacity hover:opacity-90 disabled:opacity-40"
          >
            New tracker
          </button>
        </form>

        {api.error !== null && <InlineError message={api.error} />}

        {api.loading && api.timers.length === 0 ? (
          <SkeletonRows rows={3} />
        ) : api.timers.length === 0 ? (
          <div className="rounded-xl border border-dashed border-line p-8">
            <EmptyState
              title="No trackers yet"
              hint="Name one above and it starts counting when you press Start."
            />
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3">
            {api.timers.map((timer) => (
              <StopwatchCard
                key={timer.id}
                timer={timer}
                now={api.now}
                onStart={api.start}
                onPause={api.pause}
                onReset={api.reset}
                onRename={api.rename}
                onDelete={api.remove}
              />
            ))}
          </div>
        )}
      </main>
    </div>
  );
}
