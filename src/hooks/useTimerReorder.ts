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

/** One list of cards on the page: no section (`key` null) or one section. */
export interface TimerList {
  key: string | null;
  /** The section's name, for announcements; null for no section. */
  name: string | null;
  ids: string[];
}

/** Where a dragged card would land: a list, and a slot in its reading order. */
export type TimerDropTarget =
  | ({ list: string | null; empty: false } & TimerInsertion)
  | { list: string | null; empty: true; slot: 0 };

/** Lists are keyed by section id; the cards in no section use this key. */
const LOOSE = "";
const keyOf = (list: string | null): string => list ?? LOOSE;

/** Native grip dragging keeps the layout still until drop. Menu moves use FLIP
 * once the authoritative order arrives, so both neighbours visibly swap. A
 * card dragged into another list moves to that section at the slot shown. */
export function useTimerReorder(api: TimersApi, lists: TimerList[]) {
  const scrollRef = useRef<HTMLElement>(null);
  const cardElements = useRef(new Map<string, HTMLElement>());
  /** Each list's grid (or its empty drop box), which an indicator is drawn in. */
  const listElements = useRef(new Map<string, HTMLElement>());
  /** Each list's whole area, heading included, that the pointer picks it by. */
  const zoneElements = useRef(new Map<string, HTMLElement>());
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [target, setTarget] = useState<TimerDropTarget | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const activeId = useRef<string | null>(null);
  const targetRef = useRef<TimerDropTarget | null>(null);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const scrollFrame = useRef<number | null>(null);
  const liftFrame = useRef<number | null>(null);
  const beforeMove = useRef(new Map<string, DOMRect>());
  const animations = useRef<Animation[]>([]);
  const order = lists
    .map((list) => `${keyOf(list.key)}:${list.ids.join(",")}`)
    .join("|");

  const clearTarget = useCallback(() => {
    if (scrollFrame.current !== null) cancelAnimationFrame(scrollFrame.current);
    scrollFrame.current = null;
    pointer.current = null;
    targetRef.current = null;
    setTarget(null);
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

  const listOf = (id: string): TimerList | undefined =>
    lists.find((list) => list.ids.includes(id));

  /** Every id in page order once `list` reads `ids`, for `api.reorder`. */
  const pageOrder = (list: TimerList, ids: string[]): string[] =>
    lists.flatMap((each) => (each.key === list.key ? ids : each.ids));

  const where = (list: TimerList, position: number, of: number): string =>
    `${list.name === null ? "no section" : list.name}, position ${position} of ${of}`;

  /** Moves a card within its own list, by the menu or the arrow keys. */
  const move = (id: string, delta: number) => {
    const list = listOf(id);
    if (!list) return;
    const ids = [...list.ids];
    const from = ids.indexOf(id);
    const to = from + delta;
    if (from < 0 || to < 0 || to >= ids.length) return;
    const label = api.timers.find((t) => t.id === id)?.label ?? "Tracker";
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
    api.reorder(pageOrder(list, ids));
    setAnnouncement(`${label} moved to ${where(list, to + 1, ids.length)}`);
  };

  /** The list whose area holds `y`, else the nearest one. */
  const listAt = (y: number): TimerList | null => {
    let best: { list: TimerList; distance: number } | null = null;
    for (const list of lists) {
      const zone = zoneElements.current.get(keyOf(list.key));
      if (!zone) continue;
      const { top, bottom } = zone.getBoundingClientRect();
      const distance = y < top ? top - y : y > bottom ? y - bottom : 0;
      if (best === null || distance < best.distance) best = { list, distance };
    }
    return best?.list ?? null;
  };

  const updateTarget = (x: number, y: number) => {
    if (activeId.current === null) return;
    const list = listAt(y);
    const container = list && listElements.current.get(keyOf(list.key));
    if (!list || !container) return;
    const bounds = container.getBoundingClientRect();
    // Keep the padded drop zones above/below the lists, but clear immediately
    // when the pointer leaves them sideways, even inside the scroll area.
    if (x < bounds.left || x > bounds.right) {
      clearTarget();
      return;
    }
    let next: TimerDropTarget;
    if (list.ids.length === 0) {
      next = { list: list.key, empty: true, slot: 0 };
    } else {
      const cards: DOMRect[] = [];
      for (const id of list.ids) {
        const element = cardElements.current.get(id);
        if (!element) return;
        cards.push(element.getBoundingClientRect());
      }
      const style = getComputedStyle(container);
      const insertion = timerInsertion(
        cards,
        bounds,
        x,
        y,
        Number.parseFloat(style.columnGap),
        Number.parseFloat(style.rowGap),
      );
      if (insertion === null) return;
      next = { list: list.key, empty: false, ...insertion };
    }
    // No DOM movement during dragging and no state update for an unchanged
    // slot, including child dragenter events and the 1 Hz timer refresh.
    const current = targetRef.current;
    if (current?.list === next.list && current.slot === next.slot) return;
    targetRef.current = next;
    setTarget(next);
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
    const dropped = targetRef.current;
    const payload = event.dataTransfer.getData("text/plain");
    clearDrag(); // Synchronously disarm before committing, even if drop repeats.
    if (payload !== id || dropped === null) return;
    const from = listOf(id);
    const into = lists.find((list) => list.key === dropped.list);
    if (!from || !into) return;
    const label = api.timers.find((t) => t.id === id)?.label ?? "Tracker";
    if (from.key !== into.key) {
      api.moveToSection(id, into.key, dropped.slot);
      setAnnouncement(
        `${label} moved to ${where(into, dropped.slot + 1, into.ids.length + 1)}`,
      );
      return;
    }
    const ids = [...from.ids];
    const index = ids.indexOf(id);
    if (dropped.slot === index || dropped.slot === index + 1) return;
    const to = dropped.slot > index ? dropped.slot - 1 : dropped.slot;
    ids.splice(index, 1);
    ids.splice(to, 0, id);
    api.reorder(pageOrder(from, ids));
    setAnnouncement(`${label} moved to ${where(from, to + 1, ids.length)}`);
  };

  /** Registers a list's grid and its area under the list's key. */
  const listRef =
    (list: string | null, kind: "list" | "zone") =>
    (element: HTMLElement | null): void => {
      const elements = kind === "list" ? listElements : zoneElements;
      if (element) elements.current.set(keyOf(list), element);
      else elements.current.delete(keyOf(list));
    };

  return {
    scrollRef,
    cardElements,
    listRef,
    draggingId,
    /** The list the dragged card started in, if a drag is on. */
    draggingFrom: draggingId === null ? undefined : listOf(draggingId)?.key,
    target,
    announcement,
    move,
    onDragStart,
    onDragOver,
    onDragLeave,
    onDrop,
    clearDrag,
  };
}
