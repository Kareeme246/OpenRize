import { useEffect, useState } from "react";
import {
  EmptyState,
  InlineError,
  PageHeader,
  SkeletonRows,
} from "../components/Page";
import { StopwatchCard } from "../components/StopwatchCard";
import { TimerGroupBar } from "../components/TimerGroupBar";
import { useCatalog } from "../hooks/useCatalog";
import { useTimerReorder } from "../hooks/useTimerReorder";
import { type TimersApi, useTimers } from "../hooks/useTimers";
import { elapsedMs, formatDuration, timerGroups } from "../lib/timers";

interface TimersProps {
  api?: TimersApi;
  revealTimer?: { id: string };
}

export function Timers({ api: propApi, revealTimer }: TimersProps = {}) {
  const defaultApi = useTimers();
  const api = propApi ?? defaultApi;
  const catalog = useCatalog();
  const [label, setLabel] = useState("");
  const trimmed = label.trim();

  const reorder = useTimerReorder(api);

  // Wait for the timer list before revealing a request from the hidden app.
  useEffect(() => {
    if (!revealTimer || api.loading) return;
    const element = document.getElementById(`timer-${revealTimer.id}`);
    element?.scrollIntoView({ block: "center" });
    element?.focus({ preventScroll: true });
  }, [revealTimer, api.loading]);

  const submit = (): void => {
    if (trimmed.length === 0) return;
    api.create(trimmed);
    setLabel("");
  };

  const runningCount = api.timers.filter(
    (timer) => timer.startedAt !== null,
  ).length;
  const groups = timerGroups(api.timers, api.now);
  const groupNames = groups.map((group) => group.name);
  const totalMs = api.timers.reduce(
    (sum, timer) => sum + elapsedMs(timer, api.now),
    0,
  );

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-canvas text-fg">
      <PageHeader title="Stopwatches">
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

      {/* The end padding lets the last row of cards scroll clear of a toast
          in the bottom-right corner. */}
      <main
        ref={reorder.scrollRef}
        onDragEnter={reorder.onDragOver}
        onDragOver={reorder.onDragOver}
        onDragLeave={reorder.onDragLeave}
        onDrop={reorder.onDrop}
        className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 pt-4 pb-32"
      >
        <div
          aria-live="polite"
          aria-atomic="true"
          role="status"
          className="sr-only"
        >
          {reorder.announcement}
        </div>
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
            className="shrink-0 rounded-lg bg-accent px-4 py-2 text-[13px] font-semibold text-accent-fg transition-opacity hover:opacity-90 disabled:opacity-40"
          >
            New tracker
          </button>
        </form>

        {api.error !== null && <InlineError message={api.error} />}

        <TimerGroupBar
          groups={groups}
          onStart={api.startGroup}
          onPause={api.pauseGroup}
        />

        {api.loading && api.timers.length === 0 ? (
          <SkeletonRows rows={3} />
        ) : api.timers.length === 0 ? (
          <div className="rounded-xl border border-dashed border-line p-8">
            <EmptyState
              title="No stopwatches yet"
              hint="Name one above and it starts counting when you press Start."
            />
          </div>
        ) : (
          <div
            ref={reorder.containerRef}
            className="relative grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3 py-2"
          >
            {reorder.indicator !== null && (
              <div
                aria-hidden="true"
                data-timer-insertion={reorder.indicator.slot}
                style={{
                  position: "absolute",
                  left: `${reorder.indicator.left}px`,
                  top: `${reorder.indicator.top}px`,
                  width: `${reorder.indicator.width}px`,
                  height: `${reorder.indicator.height}px`,
                }}
                className="pointer-events-none z-20 rounded-full bg-accent shadow-[0_0_8px_var(--accent)]"
              />
            )}
            {api.timers.map((timer, index) => {
              const isDragging = reorder.draggingId === timer.id;

              return (
                <StopwatchCard
                  key={timer.id}
                  timer={timer}
                  now={api.now}
                  isFirst={index === 0}
                  isLast={index === api.timers.length - 1}
                  isDragging={isDragging}
                  cardRef={(el) => {
                    if (el) reorder.cardElements.current.set(timer.id, el);
                    else reorder.cardElements.current.delete(timer.id);
                  }}
                  onDragStart={reorder.onDragStart}
                  onDragEnd={reorder.clearDrag}
                  onMoveUp={(id) => reorder.move(id, -1)}
                  onMoveDown={(id) => reorder.move(id, 1)}
                  onStart={api.start}
                  onPause={api.pause}
                  onReset={api.reset}
                  onClear={api.clear}
                  onRename={api.rename}
                  onSetGroup={api.setGroup}
                  groupNames={groupNames}
                  onAddSegment={api.addSegment}
                  onUpdateSegment={api.updateSegment}
                  onDeleteSegment={api.deleteSegment}
                  onSaveState={api.saveState}
                  onDelete={api.remove}
                  categories={catalog.categories}
                  projects={catalog.projects}
                />
              );
            })}
          </div>
        )}
      </main>
    </div>
  );
}
