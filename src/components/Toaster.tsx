import {
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { type Toast, type ToastTone, useToaster } from "../hooks/useToast";

/**
 * Where toasts appear: a stack in a corner of the main window, newest in
 * front and up to three showing, the older ones stepping back behind it
 * (smaller, fainter, only their edges in view). Pointing at or tabbing into
 * the stack fans it out so each card reads in full, and it settles back on
 * leaving. A card can be swiped away sideways: it follows the drag and flies
 * off past a distance or speed, or springs back. The default corner,
 * bottom-right, The default, bottom-right, keeps clear of the sidebar and its
 * tracking pill, of every page's header controls, and of the break panel and
 * its corner pill (separate windows at the top of the screen). It sits on
 * `z-40`, above page menus (`z-30`) and below pickers and tooltips; modal
 * sheets and dialogs are on the top layer, so a toast never covers one. A
 * side panel that must stay readable, like the entry review sheet, carries
 * `data-toast-avoid`, and the card steps beside it (or above, without room).
 *
 * Every card counts down on its own, and all of them pause while the stack is
 * fanned out, being dragged, or the window is in the background, so none can
 * vanish mid-read or mid-click. Toasts raised meanwhile wait until it is let
 * go, so nothing shifts under the pointer; ones past the three wait their
 * turn behind. F8 moves focus to the newest, Escape there dismisses it, and
 * focus goes back to where it was; a toast never takes focus on its own.
 * Screen readers hear each through a polite status region, or an assertive
 * alert for an error.
 */

export type ToastCorner =
  | "top-left"
  | "top-right"
  | "bottom-left"
  | "bottom-right";
/** `slide` moves in from the corner's edge; `fade` only fades. */
export type ToastMotion = "slide" | "fade";

/** Space from the window's edge and from a panel it avoids, in rem. */
const EDGE_REM = 1;
/** Top corners clear the title bar (`h-11`). */
const TOP_REM = 3.5;

interface Insets {
  x: number;
  y: number;
}

/**
 * The card's distance from its corner's two edges: `EDGE` from the window,
 * moved sideways past any `avoid` panel it would cover, or above or below it
 * when there is no room beside it.
 */
export function placeToast(
  corner: ToastCorner,
  width: number,
  height: number,
  avoid: DOMRect[],
  viewportWidth: number,
  viewportHeight: number,
  rem: number,
): Insets {
  const right = corner.endsWith("right");
  const bottom = corner.startsWith("bottom");
  const edge = EDGE_REM * rem;
  const insets = { x: edge, y: bottom ? edge : TOP_REM * rem };
  const box = (): { left: number; top: number } => ({
    left: right ? viewportWidth - insets.x - width : insets.x,
    top: bottom ? viewportHeight - insets.y - height : insets.y,
  });
  for (const panel of avoid) {
    const { left, top } = box();
    const overlaps =
      left < panel.right &&
      left + width > panel.left &&
      top < panel.bottom &&
      top + height > panel.top;
    if (!overlaps) continue;
    const beside = right
      ? viewportWidth - panel.left + edge
      : panel.right + edge;
    if (beside + width + edge <= viewportWidth) insets.x = beside;
    else
      insets.y = bottom
        ? viewportHeight - panel.top + edge
        : panel.bottom + edge;
  }
  return insets;
}

/** Cards on show; older ones wait behind until one of these goes. */
const VISIBLE = 3;
/** How much of each card behind shows past the one in front, in px. */
const PEEK_PX = 14;
/** How much smaller each step back is. */
const SHRINK = 0.05;
/** Space between cards when fanned out, in px. */
const GAP_PX = 10;
/** A drag this far, in px, or this fast, in px per ms, throws a card away. */
const SWIPE_PX = 60;
const SWIPE_SPEED = 0.11;
/** How far a press moves before it is a drag rather than a click, in px. */
const DRAG_SLOP_PX = 4;
/** How long a card takes to move or leave, matching `duration-300`. */
const EXIT_MS = 300;
/** After a pause, at least this long remains, so leaving it never snaps it shut. */
const RESUME_MS = 1500;
const FOCUS_KEY = "F8";

interface ToasterProps {
  corner?: ToastCorner;
  motion?: ToastMotion;
}

/** Where a card sits in the stack. */
interface Spot {
  index: number;
  y: number;
  scale: number;
}

/** A dismissed card, kept on screen while it leaves. */
interface Leaving {
  toast: Toast;
  spot: Spot;
  /** The side a swipe threw it to, or null when it simply closed. */
  swipe: -1 | 1 | null;
}

function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = (): void => setReduced(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return reduced;
}

export function Toaster({
  corner = "bottom-right",
  motion = "slide",
}: ToasterProps) {
  const { toasts, dismiss, hold } = useToaster();
  const reduced = useReducedMotion();
  const slide = motion === "slide" && !reduced;
  const right = corner.endsWith("right");
  const bottom = corner.startsWith("bottom");
  /** Away from the corner's edge: up for the bottom corners. */
  const away = bottom ? -1 : 1;

  const [leaving, setLeaving] = useState<Leaving[]>([]);
  const [heights, setHeights] = useState<Record<number, number>>({});
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [windowActive, setWindowActive] = useState(() => document.hasFocus());
  const expanded = hovered || focused;
  const empty = toasts.length === 0 && leaving.length === 0;
  /** Where focus was before F8 moved it into the stack. */
  const returnFocus = useRef<HTMLElement | null>(null);
  const listRef = useRef<HTMLOListElement | null>(null);

  useEffect(() => {
    const onFocus = (): void => setWindowActive(true);
    const onBlur = (): void => setWindowActive(false);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);

  // A stack that empties under the pointer never hears it leave.
  useEffect(() => {
    if (!empty) return;
    setHovered(false);
    setFocused(false);
    setDragging(false);
  }, [empty]);

  // Toasts raised while the viewer is in the stack wait until they are done.
  useEffect(() => {
    hold(expanded || dragging);
  }, [expanded, dragging, hold]);
  useEffect(() => () => hold(false), [hold]);

  const shown = toasts.slice(0, VISIBLE);
  const heightOf = (toast: Toast | undefined): number =>
    toast ? (heights[toast.id] ?? 0) : 0;
  const front = heightOf(shown[0]);
  const offsets: number[] = [];
  let fanned = 0;
  for (const toast of shown) {
    offsets.push(fanned);
    fanned += heightOf(toast) + GAP_PX;
  }
  const fannedExtent = Math.max(0, fanned - GAP_PX);
  const stackedExtent = front + Math.max(0, shown.length - 1) * PEEK_PX;

  const spotOf = (index: number): Spot => {
    // The ones waiting their turn sit unseen behind the last one shown.
    const step = Math.min(index, VISIBLE - 1);
    return expanded
      ? { index, y: away * (offsets[step] ?? 0), scale: 1 }
      : { index, y: away * step * PEEK_PX, scale: 1 - step * SHRINK };
  };

  useEffect(() => {
    if (toasts.length === 0) return;
    const onKeyDown = (event: globalThis.KeyboardEvent): void => {
      const card = listRef.current?.querySelector<HTMLElement>(
        `[data-toast-id="${toasts[0].id}"]`,
      );
      if (event.key !== FOCUS_KEY || !card) return;
      event.preventDefault();
      if (!listRef.current?.contains(document.activeElement)) {
        returnFocus.current =
          document.activeElement instanceof HTMLElement
            ? document.activeElement
            : null;
      }
      // The action first: it is what the keyboard came for.
      (
        card.querySelector<HTMLElement>("[data-toast-action]") ??
        card.querySelector<HTMLElement>("button")
      )?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [toasts]);

  const close = (toast: Toast, swipe: -1 | 1 | null = null): void => {
    const index = toasts.findIndex((candidate) => candidate.id === toast.id);
    if (index === -1) return;
    // Dismissing from the keyboard hands focus back instead of dropping it.
    const active = document.activeElement;
    const card = listRef.current?.querySelector(
      `[data-toast-id="${toast.id}"]`,
    );
    if (active instanceof HTMLElement && card?.contains(active)) {
      const target = returnFocus.current;
      if (target?.isConnected) target.focus();
      else active.blur();
      returnFocus.current = null;
    }
    const spot = spotOf(index);
    setLeaving((previous) =>
      previous.some((entry) => entry.toast.id === toast.id)
        ? previous
        : [...previous, { toast, spot, swipe }],
    );
    window.setTimeout(() => {
      setLeaving((previous) =>
        previous.filter((entry) => entry.toast.id !== toast.id),
      );
      setHeights(({ [toast.id]: _, ...rest }) => rest);
    }, EXIT_MS);
    dismiss(toast.id);
  };

  // Placed before paint, and again whenever the page under it changes, so it
  // steps aside as soon as a panel it must not cover opens. It is placed for
  // its fanned-out size, so fanning out never moves it.
  const [insets, setInsets] = useState<Insets | null>(null);
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list === null || empty) return;
    const place = (): void => {
      const avoid = [
        ...document.querySelectorAll<HTMLElement>("[data-toast-avoid]"),
      ]
        .map((element) => element.getBoundingClientRect())
        .filter((rect) => rect.width > 0 && rect.height > 0);
      const next = placeToast(
        corner,
        list.offsetWidth,
        fannedExtent,
        avoid,
        document.documentElement.clientWidth,
        document.documentElement.clientHeight,
        Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
      );
      setInsets((previous) =>
        previous?.x === next.x && previous.y === next.y ? previous : next,
      );
    };
    place();
    const observer = new MutationObserver(place);
    observer.observe(document.body, { childList: true, subtree: true });
    window.addEventListener("resize", place);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", place);
    };
  }, [corner, empty, fannedExtent]);

  // One list in a fixed order, newest first, so a card that starts leaving
  // stays the same element and its move carries on where it was.
  const cards = [
    ...toasts.map((toast, index) => ({
      toast,
      spot: spotOf(index),
      leaving: null,
    })),
    ...leaving.map((entry) => ({ ...entry, leaving: entry })),
  ].sort((a, b) => b.toast.id - a.toast.id);

  const newest = toasts[0];
  const announced = newest?.tone === "error" ? null : newest;
  const alerted = newest?.tone === "error" ? newest : null;

  return (
    <>
      {!empty && (
        <ol
          ref={listRef}
          aria-label="Notifications"
          onPointerEnter={() => setHovered(true)}
          onPointerLeave={() => setHovered(false)}
          onFocus={() => setFocused(true)}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget)) {
              setFocused(false);
            }
          }}
          style={{
            [right ? "right" : "left"]: insets?.x ?? `${EDGE_REM}rem`,
            [bottom ? "bottom" : "top"]:
              insets?.y ?? `${bottom ? EDGE_REM : TOP_REM}rem`,
            height: expanded ? fannedExtent : stackedExtent,
          }}
          className="fixed z-40 w-[22rem] max-w-[calc(100vw-2rem)]"
        >
          {cards.map(({ toast, spot, leaving: exit }) => (
            <ToastCard
              key={toast.id}
              toast={toast}
              spot={spot}
              exit={exit}
              bottom={bottom}
              expanded={expanded}
              frontHeight={front}
              slide={slide}
              reduced={reduced}
              paused={expanded || dragging || !windowActive}
              onHeight={(height) =>
                setHeights((previous) =>
                  previous[toast.id] === height
                    ? previous
                    : { ...previous, [toast.id]: height },
                )
              }
              onDragging={setDragging}
              onClose={(swipe) => close(toast, swipe)}
            />
          ))}
        </ol>
      )}
      {/* Always mounted, so a new message is read as a change. */}
      <div role="status" className="sr-only">
        {announced && <span key={announced.id}>{announced.message}</span>}
      </div>
      <div role="alert" className="sr-only">
        {alerted && <span key={alerted.id}>{alerted.message}</span>}
      </div>
    </>
  );
}

interface ToastCardProps {
  toast: Toast;
  spot: Spot;
  /** Set once it has been dismissed and is on its way out. */
  exit: Leaving | null;
  bottom: boolean;
  expanded: boolean;
  /** Cards behind take the front card's height, so only their edges show. */
  frontHeight: number;
  slide: boolean;
  reduced: boolean;
  paused: boolean;
  onHeight: (height: number) => void;
  onDragging: (dragging: boolean) => void;
  onClose: (swipe: -1 | 1 | null) => void;
}

function ToastCard({
  toast,
  spot,
  exit,
  bottom,
  expanded,
  frontHeight,
  slide,
  reduced,
  paused,
  onHeight,
  onDragging,
  onClose,
}: ToastCardProps) {
  const waiting = spot.index >= VISIBLE;
  const behind = spot.index > 0 && !expanded;
  const remaining = useRef(toast.duration);

  // Count down only while on show and unpaused, keeping what is left.
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    if (exit || paused || waiting) return;
    const started = performance.now();
    const wait = Math.max(remaining.current, RESUME_MS);
    remaining.current = wait;
    const timer = window.setTimeout(() => closeRef.current(null), wait);
    return () => {
      window.clearTimeout(timer);
      remaining.current -= performance.now() - started;
    };
  }, [exit, paused, waiting]);

  // Its own height, whatever the stack squeezes it to.
  const cardRef = useRef<HTMLLIElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const heightRef = useRef(onHeight);
  heightRef.current = onHeight;
  useLayoutEffect(() => {
    const card = cardRef.current;
    const content = contentRef.current;
    if (card === null || content === null) return;
    const measure = (): void =>
      heightRef.current(
        content.offsetHeight + card.offsetHeight - card.clientHeight,
      );
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  // Swiping: the card follows the pointer sideways, then flies off or
  // springs back.
  const drag = useRef<{ x: number; at: number; moved: boolean } | null>(null);
  const [dx, setDx] = useState(0);
  const [dragging, setDragging] = useState(false);
  const draggingRef = useRef(onDragging);
  draggingRef.current = onDragging;
  useEffect(() => {
    draggingRef.current(dragging);
  }, [dragging]);
  useEffect(() => () => draggingRef.current(false), []);

  const onPointerDown = (event: PointerEvent<HTMLLIElement>): void => {
    if (exit || event.button !== 0) return;
    // A press on a button is a click, never a drag.
    if (event.target instanceof Element && event.target.closest("button"))
      return;
    drag.current = { x: event.clientX, at: performance.now(), moved: false };
    event.currentTarget.setPointerCapture(event.pointerId);
  };
  const onPointerMove = (event: PointerEvent<HTMLLIElement>): void => {
    const start = drag.current;
    if (start === null) return;
    const offset = event.clientX - start.x;
    if (!start.moved && Math.abs(offset) < DRAG_SLOP_PX) return;
    start.moved = true;
    setDragging(true);
    setDx(offset);
  };
  const onPointerUp = (event: PointerEvent<HTMLLIElement>): void => {
    const start = drag.current;
    drag.current = null;
    if (start === null || !start.moved) return;
    setDragging(false);
    const offset = event.clientX - start.x;
    const speed = Math.abs(offset) / (performance.now() - start.at);
    if (Math.abs(offset) >= SWIPE_PX || speed > SWIPE_SPEED) {
      setDx(offset);
      onClose(offset < 0 ? -1 : 1);
    } else {
      setDx(0);
    }
  };
  const onPointerCancel = (): void => {
    drag.current = null;
    setDragging(false);
    setDx(0);
  };

  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key !== "Escape") return;
    // Only this toast: not a page's own Escape (review mode, a menu) as well.
    event.preventDefault();
    event.stopPropagation();
    onClose(null);
  };

  const width = cardRef.current?.offsetWidth ?? 0;
  // A swiped card leaves the way it was thrown; a closed one sinks back into
  // its edge. Reduced motion only fades either way.
  const x = exit?.swipe && !reduced ? `${exit.swipe * 120}%` : `${dx}px`;
  const sink = exit && !exit.swipe && slide ? (bottom ? 1 : -1) * 24 : 0;
  const style: CSSProperties = {
    transform: `translateX(${x}) translateY(${spot.y + sink}px) scale(${spot.scale})`,
    transformOrigin: bottom ? "center top" : "center bottom",
    zIndex: exit ? VISIBLE + 1 : VISIBLE - Math.min(spot.index, VISIBLE),
    height: behind && frontHeight > 0 ? frontHeight : undefined,
  };
  if (exit || waiting) style.opacity = 0;
  else if (dragging && width > 0)
    style.opacity = Math.max(0.2, 1 - Math.abs(dx) / width);
  else if (behind) style.opacity = 1 - spot.index * 0.15;

  const enter = slide
    ? bottom
      ? "starting:translate-y-full"
      : "starting:-translate-y-full"
    : "";
  const moves = dragging
    ? "transition-none"
    : reduced
      ? "transition-opacity"
      : "transition-[transform,opacity,height,translate]";

  return (
    // biome-ignore lint/a11y/noNoninteractiveElementInteractions: the pointer handlers are the swipe gesture; the close button and Escape dismiss it from the keyboard
    <li
      ref={cardRef}
      data-toast-id={toast.id}
      aria-label="Notification"
      aria-hidden={waiting || undefined}
      inert={exit !== null || waiting}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
      onKeyDown={onKeyDown}
      style={style}
      className={`absolute inset-x-0 ${bottom ? "bottom-0" : "top-0"} touch-pan-y select-none overflow-hidden rounded-xl border border-line-strong bg-panel text-fg shadow-black/30 shadow-xl duration-300 ease-out starting:opacity-0 sharper:rounded-md ${moves} ${enter} ${
        exit || waiting ? "pointer-events-none" : ""
      }`}
    >
      <div
        ref={contentRef}
        className={`p-3 transition-opacity duration-300 ${behind ? "opacity-0" : ""}`}
      >
        <div className="flex items-start gap-2.5">
          <ToneGlyph tone={toast.tone} />
          <p className="min-w-0 flex-1 py-0.5 font-medium text-[12.5px] text-fg-strong leading-snug [overflow-wrap:anywhere]">
            {toast.message}
          </p>
          <button
            type="button"
            onClick={() => onClose(null)}
            aria-label="Dismiss"
            className="-mt-0.5 -mr-0.5 grid size-6 shrink-0 place-items-center rounded-md text-fg-faint outline-accent hover:bg-surface-strong hover:text-fg focus-visible:outline-2"
          >
            <svg
              viewBox="0 0 24 24"
              className="size-3.5"
              fill="none"
              stroke="currentColor"
              strokeWidth={2.2}
              strokeLinecap="round"
              aria-hidden="true"
            >
              <path d="M6 6l12 12M18 6L6 18" />
            </svg>
          </button>
        </div>
        {toast.action && (
          <div className="mt-2.5 flex justify-end">
            <button
              type="button"
              data-toast-action
              onClick={() => {
                toast.action?.onAction();
                onClose(null);
              }}
              className="rounded-md bg-accent px-2.5 py-1 font-semibold text-[12px] text-accent-fg outline-accent outline-offset-2 transition-opacity hover:opacity-90 focus-visible:outline-2 sharper:rounded"
            >
              {toast.action.label}
            </button>
          </div>
        )}
      </div>
    </li>
  );
}

const GLYPH_STYLES: Record<ToastTone, string> = {
  neutral: "bg-surface-strong text-fg-muted",
  success: "bg-success-soft text-success",
  error: "bg-danger-soft text-danger",
};

/** An i for a plain fact, a tick for a finished job, a bang for a failure. */
const GLYPH_PATHS: Record<ToastTone, string> = {
  neutral: "M12 11v5.5M12 7.5h.01",
  success: "M7.5 12.5l3 3 6-6.5",
  error: "M12 7.5v5.5M12 16.5h.01",
};

function ToneGlyph({ tone }: { tone: ToastTone }) {
  return (
    <span
      aria-hidden="true"
      className={`grid size-6 shrink-0 place-items-center rounded-md sharper:rounded ${GLYPH_STYLES[tone]}`}
    >
      <svg
        viewBox="0 0 24 24"
        className="size-4"
        fill="none"
        stroke="currentColor"
        strokeWidth={2.4}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path d={GLYPH_PATHS[tone]} />
      </svg>
    </span>
  );
}
