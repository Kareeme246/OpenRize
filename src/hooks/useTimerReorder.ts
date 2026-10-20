import {
  type DragEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import type { TimersApi } from "./useTimers";

interface InsertionIndicator {
  slot: number;
  left: number;
  top: number;
  width: number;
}

/** Native grip dragging keeps the layout still until drop. Menu moves use FLIP
 * once the authoritative order arrives, so both neighbours visibly swap. */
export function useTimerReorder(api: TimersApi) {
  const containerRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLElement>(null);
  const cardElements = useRef(new Map<string, HTMLElement>());
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [indicator, setIndicator] = useState<InsertionIndicator | null>(null);
  const activeId = useRef<string | null>(null);
  const slot = useRef<number | null>(null);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const scrollFrame = useRef<number | null>(null);
  const liftFrame = useRef<number | null>(null);
  const beforeMove = useRef(new Map<string, DOMRect>());
  const animations = useRef<Animation[]>([]);
  const order = api.timers.map((timer) => timer.id).join(",");

  const clearTarget = useCallback(() => {
    if (scrollFrame.current !== null) cancelAnimationFrame(scrollFrame.current);
    scrollFrame.current = null;
    pointer.current = null;
    slot.current = null;
    setIndicator(null);
  }, []);

  const clearDrag = useCallback(() => {
    activeId.current = null;
    if (liftFrame.current !== null) cancelAnimationFrame(liftFrame.current);
    liftFrame.current = null;
    setDraggingId(null);
    clearTarget();
  }, [clearTarget]);

  useEffect(() => {
    const onEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") clearDrag();
    };
    // WebKit can report null relatedTarget when crossing a child. Only a
    // viewport exit clears here; the list handler covers leaving the list.
    const leaveWindow = (event: globalThis.DragEvent) => {
      if (
        event.relatedTarget === null &&
        (event.clientX <= 0 ||
          event.clientY <= 0 ||
          event.clientX >= window.innerWidth ||
          event.clientY >= window.innerHeight)
      )
        clearTarget();
    };
    window.addEventListener("keydown", onEscape);
    window.addEventListener("dragend", clearDrag);
    window.addEventListener("drop", clearDrag);
    window.addEventListener("blur", clearDrag);
    window.addEventListener("dragleave", leaveWindow);
    return () => {
      window.removeEventListener("keydown", onEscape);
      window.removeEventListener("dragend", clearDrag);
      window.removeEventListener("drop", clearDrag);
      window.removeEventListener("blur", clearDrag);
      window.removeEventListener("dragleave", leaveWindow);
      if (scrollFrame.current !== null)
        cancelAnimationFrame(scrollFrame.current);
      if (liftFrame.current !== null) cancelAnimationFrame(liftFrame.current);
      for (const animation of animations.current) animation.cancel();
    };
  }, [clearDrag, clearTarget]);

  // External creation/deletion/reordering invalidates an in-flight drop slot.
  useEffect(() => {
    void order;
    clearDrag();
  }, [order, clearDrag]);

  useLayoutEffect(() => {
    void order;
    const previous = beforeMove.current;
    beforeMove.current = new Map();
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    for (const [id, before] of previous) {
      const element = cardElements.current.get(id);
      if (!element) continue;
      const after = element.getBoundingClientRect();
      const x = before.left - after.left;
      const y = before.top - after.top;
      if (x === 0 && y === 0) continue;
      const animation = element.animate(
        [
          { transform: `translate(${x}px, ${y}px)` },
          { transform: "translate(0, 0)" },
        ],
        { duration: 180, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
      );
      animations.current.push(animation);
    }
  }, [order]);

  const move = (id: string, delta: number) => {
    const ids = api.timers.map((timer) => timer.id);
    const from = ids.indexOf(id);
    const to = from + delta;
    if (from < 0 || to < 0 || to >= ids.length) return;
    clearDrag();
    const previous = new Map<string, DOMRect>();
    for (const affected of [ids[from], ids[to]]) {
      const element = cardElements.current.get(affected);
      if (element) previous.set(affected, element.getBoundingClientRect());
    }
    for (const animation of animations.current) animation.cancel();
    animations.current = [];
    beforeMove.current = previous;
    [ids[from], ids[to]] = [ids[to], ids[from]];
    api.reorder(ids);
  };

  const updateTarget = (x: number, y: number) => {
    const container = containerRef.current;
    if (!container || activeId.current === null) return;
    const items = api.timers.flatMap((timer, index) => {
      const element = cardElements.current.get(timer.id);
      return element ? [{ index, rect: element.getBoundingClientRect() }] : [];
    });
    if (items.length === 0) return;
    // Nearest rectangle also handles gaps and the padded space above/below
    // the grid. Within a card, its upper/lower half selects before/after.
    const first = items[0];
    const last = items[items.length - 1];
    let target = first;
    let distance = Number.POSITIVE_INFINITY;
    for (const item of items) {
      const dx = Math.max(item.rect.left - x, 0, x - item.rect.right);
      const dy = Math.max(item.rect.top - y, 0, y - item.rect.bottom);
      const nextDistance = dx * dx + dy * dy;
      if (nextDistance < distance) {
        distance = nextDistance;
        target = item;
      }
    }
    if (y < first.rect.top) target = first;
    if (y > last.rect.bottom) target = last;
    const after = y >= target.rect.top + target.rect.height / 2;
    const nextSlot = target.index + (after ? 1 : 0);
    // No DOM movement during dragging and no state update for an unchanged
    // slot, including child dragenter events and the 1 Hz timer refresh.
    if (slot.current === nextSlot) return;
    slot.current = nextSlot;
    const bounds = container.getBoundingClientRect();
    setIndicator({
      slot: nextSlot,
      left: target.rect.left - bounds.left,
      top:
        (after ? target.rect.bottom + 6 : target.rect.top - 6) - bounds.top - 1,
      width: target.rect.width,
    });
  };

  const scroll = () => {
    scrollFrame.current = null;
    const element = scrollRef.current;
    const point = pointer.current;
    if (!element || !point || activeId.current === null) return;
    const bounds = element.getBoundingClientRect();
    const edge = Math.min(64, bounds.height / 4);
    const speed =
      point.y < bounds.top + edge
        ? -12 * Math.min(1, (bounds.top + edge - point.y) / edge)
        : point.y > bounds.bottom - edge
          ? 12 * Math.min(1, (point.y - bounds.bottom + edge) / edge)
          : 0;
    if (speed === 0) return;
    element.scrollTop += speed;
    updateTarget(point.x, point.y);
    scrollFrame.current = requestAnimationFrame(scroll);
  };

  const onDragStart = (event: DragEvent, id: string) => {
    const card = cardElements.current.get(id);
    if (!card) {
      event.preventDefault();
      return;
    }
    clearDrag();
    for (const animation of animations.current) animation.cancel();
    animations.current = [];
    beforeMove.current.clear();
    event.dataTransfer.clearData();
    event.dataTransfer.setData("text/plain", id);
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.dropEffect = "move";
    const rect = card.getBoundingClientRect();
    // Capture the full, opaque card, not the grip, SVG or a text selection.
    event.dataTransfer.setDragImage(
      card,
      event.clientX - rect.left,
      event.clientY - rect.top,
    );
    activeId.current = id;
    liftFrame.current = requestAnimationFrame(() => {
      liftFrame.current = null;
      if (activeId.current === id) setDraggingId(id);
    });
  };

  const onDragOver = (event: DragEvent<HTMLElement>) => {
    if (activeId.current === null) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    pointer.current = { x: event.clientX, y: event.clientY };
    updateTarget(event.clientX, event.clientY);
    if (scrollFrame.current === null)
      scrollFrame.current = requestAnimationFrame(scroll);
  };

  const onDragLeave = (event: DragEvent<HTMLElement>) => {
    const next = event.relatedTarget;
    if (next instanceof Node && event.currentTarget.contains(next)) return;
    // Some WebKit child transitions have no relatedTarget. Hit-test before
    // clearing so crossing the ring, label or menu cannot blink the line.
    const underPointer = document.elementFromPoint(
      event.clientX,
      event.clientY,
    );
    if (underPointer && event.currentTarget.contains(underPointer)) return;
    clearTarget();
  };

  const onDrop = (event: DragEvent<HTMLElement>) => {
    const id = activeId.current;
    if (id === null) return;
    event.preventDefault();
    event.stopPropagation();
    updateTarget(event.clientX, event.clientY);
    const targetSlot = slot.current;
    const payload = event.dataTransfer.getData("text/plain");
    clearDrag(); // Synchronously disarm before committing, even if drop repeats.
    if (payload !== id || targetSlot === null) return;
    const ids = api.timers.map((timer) => timer.id);
    const from = ids.indexOf(id);
    if (from < 0 || targetSlot === from || targetSlot === from + 1) return;
    ids.splice(from, 1);
    ids.splice(targetSlot > from ? targetSlot - 1 : targetSlot, 0, id);
    api.reorder(ids);
  };

  return {
    containerRef,
    scrollRef,
    cardElements,
    draggingId,
    indicator,
    move,
    onDragStart,
    onDragOver,
    onDragLeave,
    onDrop,
    clearDrag,
  };
}
