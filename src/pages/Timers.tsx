import { useCallback, useEffect, useRef, useState } from "react";
import {
  EmptyState,
  InlineError,
  PageHeader,
  SkeletonRows,
} from "../components/Page";
import { StopwatchCard } from "../components/StopwatchCard";
import { useCatalog } from "../hooks/useCatalog";
import { type TimersApi, useTimers } from "../hooks/useTimers";
import { elapsedMs, formatDuration } from "../lib/timers";

interface TimersProps {
  api?: TimersApi;
  revealTimer?: { id: string };
}

interface InsertionIndicator {
  orientation: "horizontal" | "vertical";
  left: number;
  top: number;
  width: number;
  height: number;
}

export function Timers({ api: propApi, revealTimer }: TimersProps = {}) {
  const defaultApi = useTimers();
  const api = propApi ?? defaultApi;
  const catalog = useCatalog();
  const [label, setLabel] = useState("");
  const trimmed = label.trim();

  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [armedId, setArmedId] = useState<string | null>(null);
  const [indicator, setIndicator] = useState<InsertionIndicator | null>(null);

  const containerRef = useRef<HTMLDivElement>(null);
  const cardElements = useRef<Map<string, HTMLElement>>(new Map());
  const draggingIdRef = useRef<string | null>(null);
  draggingIdRef.current = draggingId;
  const dropSlotRef = useRef<number | null>(null);

  // Wait for the timer list before revealing a request from the hidden app.
  useEffect(() => {
    if (!revealTimer || api.loading) return;
    const element = document.getElementById(`timer-${revealTimer.id}`);
    element?.scrollIntoView({ block: "center" });
    element?.focus({ preventScroll: true });
  }, [revealTimer, api.loading]);

  // Clean up armed state on window pointerup
  useEffect(() => {
    const onWindowPointerUp = () => setArmedId(null);
    window.addEventListener("pointerup", onWindowPointerUp);
    return () => window.removeEventListener("pointerup", onWindowPointerUp);
  }, []);

  const submit = (): void => {
    if (trimmed.length === 0) return;
    api.create(trimmed);
    setLabel("");
  };

  const handleDragStart = useCallback((event: React.DragEvent, id: string) => {
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", id);

    const cardEl = cardElements.current.get(id);
    if (cardEl) {
      const rect = cardEl.getBoundingClientRect();
      event.dataTransfer.setDragImage(
        cardEl,
        event.clientX - rect.left,
        event.clientY - rect.top,
      );
    }

    draggingIdRef.current = id;
    // Defer dimming the card so the browser captures the crisp drag image first
    requestAnimationFrame(() => {
      setDraggingId(id);
    });
  }, []);

  const handleDragEnd = useCallback(() => {
    setDraggingId(null);
    setArmedId(null);
    setIndicator(null);
    dropSlotRef.current = null;
  }, []);

  const handleDragOver = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";

      const container = containerRef.current;
      if (!container || api.timers.length === 0) return;

      const containerRect = container.getBoundingClientRect();
      const clientX = event.clientX;
      const clientY = event.clientY;

      const timerIds = api.timers.map((t) => t.id);
      const items: { id: string; index: number; rect: DOMRect }[] = [];
      for (let i = 0; i < timerIds.length; i++) {
        const el = cardElements.current.get(timerIds[i]);
        if (el) {
          items.push({
            id: timerIds[i],
            index: i,
            rect: el.getBoundingClientRect(),
          });
        }
      }
      if (items.length === 0) return;

      // Group cards into visual rows
      interface VisualRow {
        top: number;
        bottom: number;
        cards: { id: string; index: number; rect: DOMRect }[];
      }
      const rows: VisualRow[] = [];
      for (const item of items) {
        const existing = rows.find((r) => Math.abs(r.top - item.rect.top) < 20);
        if (existing) {
          existing.cards.push(item);
          existing.top = Math.min(existing.top, item.rect.top);
          existing.bottom = Math.max(existing.bottom, item.rect.bottom);
        } else {
          rows.push({
            top: item.rect.top,
            bottom: item.rect.bottom,
            cards: [item],
          });
        }
      }
      rows.sort((a, b) => a.top - b.top);
      for (const row of rows) {
        row.cards.sort((a, b) => a.rect.left - b.rect.left);
      }

      // Determine target slot
      let targetSlot = 0;
      let isAboveAll = false;
      let isBelowAll = false;

      if (clientY < rows[0].top) {
        targetSlot = 0;
        isAboveAll = true;
      } else if (clientY > rows[rows.length - 1].bottom) {
        targetSlot = timerIds.length;
        isBelowAll = true;
      } else {
        let activeRow = rows[0];
        for (let r = 0; r < rows.length; r++) {
          const row = rows[r];
          const next = rows[r + 1];
          if (!next) {
            activeRow = row;
            break;
          }
          const rowBoundary = (row.bottom + next.top) / 2;
          if (clientY <= rowBoundary) {
            activeRow = row;
            break;
          }
        }

        const cards = activeRow.cards;
        if (cards.length === 1) {
          const card = cards[0];
          const midY = (card.rect.top + card.rect.bottom) / 2;
          targetSlot = clientY < midY ? card.index : card.index + 1;
        } else {
          if (clientX < (cards[0].rect.left + cards[0].rect.right) / 2) {
            targetSlot = cards[0].index;
          } else if (
            clientX >=
            (cards[cards.length - 1].rect.left +
              cards[cards.length - 1].rect.right) /
              2
          ) {
            targetSlot = cards[cards.length - 1].index + 1;
          } else {
            targetSlot = cards[0].index;
            for (let c = 0; c < cards.length - 1; c++) {
              const c1 = cards[c];
              const c2 = cards[c + 1];
              const mid1 = (c1.rect.left + c1.rect.right) / 2;
              const mid2 = (c2.rect.left + c2.rect.right) / 2;
              if (clientX >= mid1 && clientX < mid2) {
                targetSlot = c1.index + 1;
                break;
              }
            }
          }
        }
      }

      if (targetSlot === dropSlotRef.current) return;
      dropSlotRef.current = targetSlot;

      // Compute indicator line geometry relative to container
      let nextInd: InsertionIndicator;
      if (isAboveAll || targetSlot === 0) {
        if (isAboveAll || rows[0].cards.length === 1) {
          const topPos = Math.max(0, rows[0].top - containerRect.top - 6);
          nextInd = {
            orientation: "horizontal",
            left: 0,
            top: topPos,
            width: containerRect.width,
            height: 2,
          };
        } else {
          const firstCard = rows[0].cards[0];
          nextInd = {
            orientation: "vertical",
            left: Math.max(0, firstCard.rect.left - containerRect.left - 6),
            top: firstCard.rect.top - containerRect.top,
            width: 2,
            height: firstCard.rect.height,
          };
        }
      } else if (isBelowAll || targetSlot === timerIds.length) {
        const lastRow = rows[rows.length - 1];
        if (isBelowAll || lastRow.cards.length === 1) {
          const topPos = lastRow.bottom - containerRect.top + 6;
          nextInd = {
            orientation: "horizontal",
            left: 0,
            top: topPos,
            width: containerRect.width,
            height: 2,
          };
        } else {
          const lastCard = lastRow.cards[lastRow.cards.length - 1];
          nextInd = {
            orientation: "vertical",
            left: lastCard.rect.right - containerRect.left + 6,
            top: lastCard.rect.top - containerRect.top,
            width: 2,
            height: lastCard.rect.height,
          };
        }
      } else {
        // Between item targetSlot - 1 and item targetSlot
        const prevItem = items[targetSlot - 1];
        const nextItem = items[targetSlot];

        // Check if on same row
        if (Math.abs(prevItem.rect.top - nextItem.rect.top) < 20) {
          const gapCenterX = (prevItem.rect.right + nextItem.rect.left) / 2;
          nextInd = {
            orientation: "vertical",
            left: gapCenterX - containerRect.left - 1,
            top: prevItem.rect.top - containerRect.top,
            width: 2,
            height: prevItem.rect.height,
          };
        } else {
          // Different rows: horizontal line in the row gap
          const gapCenterY = (prevItem.rect.bottom + nextItem.rect.top) / 2;
          nextInd = {
            orientation: "horizontal",
            left: 0,
            top: gapCenterY - containerRect.top - 1,
            width: containerRect.width,
            height: 2,
          };
        }
      }

      setIndicator(nextInd);
    },
    [api.timers],
  );

  const handleDragLeave = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      if (
        event.relatedTarget &&
        event.currentTarget.contains(event.relatedTarget as Node)
      ) {
        return;
      }
      setIndicator(null);
      dropSlotRef.current = null;
    },
    [],
  );

  const handleDrop = useCallback(
    (event: React.DragEvent<HTMLDivElement>) => {
      event.preventDefault();
      const draggedId =
        draggingIdRef.current || event.dataTransfer.getData("text/plain");
      const targetSlot = dropSlotRef.current;

      setDraggingId(null);
      setArmedId(null);
      setIndicator(null);
      dropSlotRef.current = null;

      if (!draggedId || targetSlot === null) return;

      const currentOrder = api.timers.map((t) => t.id);
      const fromIdx = currentOrder.indexOf(draggedId);
      if (fromIdx === -1) return;

      if (targetSlot === fromIdx || targetSlot === fromIdx + 1) return;

      const nextOrder = [...currentOrder];
      const [moved] = nextOrder.splice(fromIdx, 1);
      const toIdx = targetSlot > fromIdx ? targetSlot - 1 : targetSlot;
      nextOrder.splice(toIdx, 0, moved);

      api.reorder(nextOrder);
    },
    [api],
  );

  const handleMoveUp = useCallback(
    (id: string) => {
      const current = api.timers.map((t) => t.id);
      const idx = current.indexOf(id);
      if (idx <= 0) return;
      const next = [...current];
      [next[idx - 1], next[idx]] = [next[idx], next[idx - 1]];
      api.reorder(next);
    },
    [api],
  );

  const handleMoveDown = useCallback(
    (id: string) => {
      const current = api.timers.map((t) => t.id);
      const idx = current.indexOf(id);
      if (idx === -1 || idx >= current.length - 1) return;
      const next = [...current];
      [next[idx], next[idx + 1]] = [next[idx + 1], next[idx]];
      api.reorder(next);
    },
    [api],
  );

  const runningCount = api.timers.filter(
    (timer) => timer.startedAt !== null,
  ).length;
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
      <main className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 pt-4 pb-32">
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
          // biome-ignore lint/a11y/noStaticElementInteractions: drag-and-drop drop zone for timer cards; keyboard reorder is on the card handle and menu
          <div
            ref={containerRef}
            onDragOver={handleDragOver}
            onDragLeave={handleDragLeave}
            onDrop={handleDrop}
            className="relative grid grid-cols-[repeat(auto-fill,minmax(240px,1fr))] gap-3"
          >
            {indicator !== null && (
              <div
                aria-hidden="true"
                style={{
                  position: "absolute",
                  left: `${indicator.left}px`,
                  top: `${indicator.top}px`,
                  width: `${indicator.width}px`,
                  height: `${indicator.height}px`,
                }}
                className="pointer-events-none z-20 rounded-full bg-accent shadow-[0_0_8px_var(--accent)]"
              />
            )}
            {api.timers.map((timer, index) => {
              const isDragging = draggingId === timer.id;
              const canDrag = armedId === timer.id;

              return (
                <StopwatchCard
                  key={timer.id}
                  timer={timer}
                  now={api.now}
                  isFirst={index === 0}
                  isLast={index === api.timers.length - 1}
                  isDragging={isDragging}
                  canDrag={canDrag}
                  cardRef={(el) => {
                    if (el) cardElements.current.set(timer.id, el);
                    else cardElements.current.delete(timer.id);
                  }}
                  onArmDrag={setArmedId}
                  onDisarmDrag={() => setArmedId(null)}
                  onDragStart={handleDragStart}
                  onDragEnd={handleDragEnd}
                  onMoveUp={handleMoveUp}
                  onMoveDown={handleMoveDown}
                  onStart={api.start}
                  onPause={api.pause}
                  onReset={api.reset}
                  onClear={api.clear}
                  onRename={api.rename}
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
