import { useEffect, useState } from "react";
import {
  EmptyState,
  InlineError,
  PageHeader,
  SkeletonRows,
} from "../components/Page";
import { StopwatchCard } from "../components/StopwatchCard";
import { TimerSectionHeading } from "../components/TimerSectionHeading";
import { useCatalog } from "../hooks/useCatalog";
import { type TimerList, useTimerReorder } from "../hooks/useTimerReorder";
import { useTimerSections } from "../hooks/useTimerSections";
import { type TimersApi, useTimers } from "../hooks/useTimers";
import {
  elapsedMs,
  formatDuration,
  type Timer,
  timerLists,
  timerTags,
} from "../lib/timers";

interface TimersProps {
  api?: TimersApi;
  revealTimer?: { id: string };
}

/** The glowing line a dragged card would drop at. */
const INSERTION_LINE =
  "pointer-events-none z-20 rounded-full bg-accent shadow-[0_0_8px_var(--accent)]";

export function Timers({ api: propApi, revealTimer }: TimersProps = {}) {
  const defaultApi = useTimers();
  const api = propApi ?? defaultApi;
  const sections = useTimerSections();
  const catalog = useCatalog();
  const [label, setLabel] = useState("");
  const trimmed = label.trim();

  const lists = timerLists(api.timers, sections.sections);
  const reorderLists: TimerList[] = lists.map((list) => ({
    key: list.section?.id ?? null,
    name: list.section?.name ?? null,
    ids: list.timers.map((timer) => timer.id),
  }));
  const reorder = useTimerReorder(api, reorderLists);
  const target = reorder.target;

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

  const addSection = (): void => {
    if (trimmed.length === 0) return;
    sections.create(trimmed);
    setLabel("");
  };

  const moveSection = (id: string, delta: number): void => {
    const ids = sections.sections.map((section) => section.id);
    const from = ids.indexOf(id);
    const to = from + delta;
    if (from < 0 || to < 0 || to >= ids.length) return;
    [ids[from], ids[to]] = [ids[to], ids[from]];
    sections.reorder(ids);
  };

  const runningCount = api.timers.filter(
    (timer) => timer.startedAt !== null,
  ).length;
  const tags = timerTags(api.timers, api.now);
  const totalMs = api.timers.reduce(
    (sum, timer) => sum + elapsedMs(timer, api.now),
    0,
  );
  const loose = lists[0].timers;
  const error = api.error ?? sections.error;

  const card = (timer: Timer, index: number, of: Timer[]) => (
    <StopwatchCard
      key={timer.id}
      timer={timer}
      now={api.now}
      isFirst={index === 0}
      isLast={index === of.length - 1}
      isDragging={reorder.draggingId === timer.id}
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
      onSetTag={api.setTag}
      tags={tags}
      onStartTag={api.startTag}
      onPauseTag={api.pauseTag}
      sections={sections.sections}
      onMoveToSection={(id, sectionId) => api.moveToSection(id, sectionId)}
      onCreateSection={sections.create}
      reserveTagRow={tags.length > 0}
      onAddSegment={api.addSegment}
      onUpdateSegment={api.updateSegment}
      onDeleteSegment={api.deleteSegment}
      onSaveState={api.saveState}
      onDelete={api.remove}
      categories={catalog.categories}
      projects={catalog.projects}
    />
  );

  /** A list's cards, with the line a dragged card would drop at. */
  const grid = (key: string | null, timers: Timer[]) => (
    <div
      ref={(element) => {
        reorder.listRef(key, "list")(element);
        if (key === null) reorder.listRef(key, "zone")(element);
      }}
      className="relative grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3 py-2"
    >
      {target !== null && !target.empty && target.list === key && (
        <div
          aria-hidden="true"
          data-timer-insertion={target.slot}
          style={{
            position: "absolute",
            left: `${target.left}px`,
            top: `${target.top}px`,
            width: `${target.width}px`,
            height: `${target.height}px`,
          }}
          className={INSERTION_LINE}
        />
      )}
      {timers.map((timer, index) => card(timer, index, timers))}
    </div>
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
            aria-label="New tracker or section name"
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
          <button
            type="button"
            disabled={trimmed.length === 0}
            onClick={addSection}
            className="shrink-0 rounded-lg border border-line bg-panel px-4 py-2 text-[13px] font-medium text-fg-soft transition-colors hover:bg-surface hover:text-fg disabled:pointer-events-none disabled:opacity-40"
          >
            New section
          </button>
        </form>

        {error !== null && <InlineError message={error} />}

        {api.loading && api.timers.length === 0 ? (
          <SkeletonRows rows={3} />
        ) : api.timers.length === 0 && sections.sections.length === 0 ? (
          <div className="rounded-xl border border-dashed border-line p-8">
            <EmptyState
              title="No stopwatches yet"
              hint="Name one above and it starts counting when you press Start."
            />
          </div>
        ) : (
          <>
            {loose.length > 0 && grid(null, loose)}
            {lists.slice(1).map((list, index, all) => {
              const section = list.section;
              if (section === null) return null;
              const into =
                target !== null &&
                target.list === section.id &&
                reorder.draggingFrom !== section.id;
              return (
                <section
                  key={section.id}
                  ref={reorder.listRef(section.id, "zone")}
                  aria-label={`Section ${section.name}`}
                  className="relative flex flex-col gap-1"
                >
                  {/* With every stopwatch in a section, the gap above the
                      first one is where a card drops out of its section. */}
                  {index === 0 && loose.length === 0 && (
                    <div
                      ref={(element) => {
                        reorder.listRef(null, "list")(element);
                        reorder.listRef(null, "zone")(element);
                      }}
                      className="absolute inset-x-0 -top-2 h-0"
                    >
                      {target !== null && target.list === null && (
                        <div
                          aria-hidden="true"
                          data-timer-insertion={0}
                          className={`absolute inset-x-0 -top-px h-0.5 ${INSERTION_LINE}`}
                        />
                      )}
                    </div>
                  )}
                  <TimerSectionHeading
                    section={section}
                    count={list.timers.length}
                    isFirst={index === 0}
                    isLast={index === all.length - 1}
                    isDropTarget={into}
                    onRename={(name) => sections.rename(section.id, name)}
                    onMoveUp={() => moveSection(section.id, -1)}
                    onMoveDown={() => moveSection(section.id, 1)}
                    onDelete={() => sections.remove(section.id)}
                  />
                  {list.timers.length > 0 ? (
                    grid(section.id, list.timers)
                  ) : (
                    <div
                      ref={reorder.listRef(section.id, "list")}
                      data-timer-section-empty={section.name}
                      className={`my-2 rounded-xl border border-dashed p-[18px] text-center text-[11.5px] transition-colors ${
                        into
                          ? "border-accent bg-accent-soft text-accent"
                          : "border-line-strong text-fg-faint"
                      }`}
                    >
                      Drag a stopwatch here, or use Move to in its menu.
                    </div>
                  )}
                </section>
              );
            })}
          </>
        )}
      </main>
    </div>
  );
}
