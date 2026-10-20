import {
  type DragEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { type TimerInsertion, timerInsertion } from "../lib/timerInsertion";
import type { TimersApi } from "./useTimers";

/** Native grip dragging keeps the layout still until drop. Menu moves use FLIP
 * once the authoritative order arrives, so both neighbours visibly swap. */
export function useTimerReorder(api: TimersApi) {
  const containerRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLElement>(null);
  const cardElements = useRef(new Map<string, HTMLElement>());
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [indicator, setIndicator] = useState<TimerInsertion | null>(null);
  const [announcement, setAnnouncement] = useState("");
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
    const timer = api.timers.find((t) => t.id === id);
    const label = timer?.label ?? "Tracker";
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
    setAnnouncement(`${label} moved to position ${to + 1} of ${ids.length}`);
  };

  const updateTarget = (x: number, y: number) => {
    const container = containerRef.current;
    if (!container || activeId.current === null) return;
    const bounds = container.getBoundingClientRect();
    // Keep the padded drop zones above/below the list, but clear immediately
    // when the pointer leaves the grid sideways, even inside the scroll area.
    if (x < bounds.left || x > bounds.right) {
      clearTarget();
      return;
    }
    const cards: DOMRect[] = [];
    for (const timer of api.timers) {
      const element = cardElements.current.get(timer.id);
      if (!element) return;
      cards.push(element.getBoundingClientRect());
    }
    const style = getComputedStyle(container);
    const next = timerInsertion(
      cards,
      bounds,
      x,
      y,
      Number.parseFloat(style.columnGap),
      Number.parseFloat(style.rowGap),
    );
    if (next === null) return;
    // No DOM movement during dragging and no state update for an unchanged
    // slot, including child dragenter events and the 1 Hz timer refresh.
    if (slot.current === next.slot) return;
    slot.current = next.slot;
    setIndicator(next);
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
    const timer = api.timers.find((t) => t.id === id);
    const label = timer?.label ?? "Tracker";
    const to = targetSlot > from ? targetSlot - 1 : targetSlot;
    ids.splice(from, 1);
    ids.splice(to, 0, id);
    api.reorder(ids);
    setAnnouncement(`${label} moved to position ${to + 1} of ${ids.length}`);
  };

  return {
    containerRef,
    scrollRef,
    cardElements,
    draggingId,
    indicator,
    announcement,
    move,
    onDragStart,
    onDragOver,
    onDragLeave,
    onDrop,
    clearDrag,
  };
}
