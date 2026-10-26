import * as RadixToast from "@radix-ui/react-toast";
import { type CSSProperties, useLayoutEffect, useRef, useState } from "react";
import { type Toast, type ToastTone, useToaster } from "../hooks/useToast";

/**
 * Where toasts appear: a stack in a corner of the main window, newest in
 * front and up to three showing, the older ones stepping back behind it
 * (smaller, fainter, only their edges in view). Pointing at or tabbing into
 * the stack fans it out so each one reads in full, and it settles back on
 * leaving.
 *
 * A card travels in from the edge its corner is anchored to (up from the
 * bottom, down from the top) and leaves back through it. As a `pill`, a toast
 * instead opens out of a small capsule, growing on a
 * spring while its content cross-fades in, and closes back into it.
 *
 * Behaviour is Radix Toast's (`@radix-ui/react-toast`), not ours: it counts
 * each toast down and pauses them all while the pointer or focus is in the
 * stack or the window is in the background, dismisses on a swipe toward the
 * anchored side, on Escape and on its close button, moves focus to the stack
 * on F8 and back to it when a focused toast closes, and announces each one
 * (assertively for an error, politely otherwise). This file only lays the
 * stack out and moves it.
 *
 * The default corner, bottom-right, keeps clear of the sidebar and its
 * tracking pill, of every page's header controls, and of the break panel and
 * its corner pill (separate windows at the top of the screen). The stack
 * always sits in its corner of the window, floating over whatever page
 * content is there (a docked side panel included) while it shows. It sits on
 * `z-40`, above page menus (`z-30`) and below pickers and tooltips; modal
 * sheets and dialogs are on the top layer, so a toast never covers one.
 * Reduced motion keeps only the fades.
 */

export type ToastCorner =
  | "top-left"
  | "top-right"
  | "bottom-left"
  | "bottom-right";
/** `slide` travels in from the anchored edge; `fade` only fades. */
export type ToastMotion = "slide" | "fade";
/**
 * `card` is a compact message; `pill` opens out of a capsule.
 */
export type ToastPresentation = "card" | "pill";

/** Space from the window's edge, in rem. */
const EDGE_REM = 1;
/**
 * Every toast is this wide, whatever it says, so the stack keeps one width; a
 * longer message wraps onto more lines instead. It narrows with a small
 * window, keeping `EDGE_REM` clear on both sides.
 */
const TOAST_WIDTH = "min(20rem, calc(100vw - 2rem))";
/** Top corners clear the title bar (`h-11`). */
const TOP_REM = 3.5;

interface Insets {
  x: number;
  y: number;
}

/**
 * The stack's distance from its corner's two edges: `EDGE` from the window,
 * and a top corner clears the title bar.
 */
export function placeToast(corner: ToastCorner, rem: number): Insets {
  return {
    x: EDGE_REM * rem,
    y: (corner.startsWith("bottom") ? EDGE_REM : TOP_REM) * rem,
  };
}

/** Toasts on show; newer ones wait in the queue until one of these goes. */
const VISIBLE = 3;
/** How much of each toast behind shows past the one in front, in px. */
const PEEK_PX = 14;
/** How much smaller each step back is. */
const SHRINK = 0.05;
/** Space between toasts when fanned out, in px. */
const GAP_PX = 10;
/** How far a swipe toward the anchored side must go to dismiss, in px. */
const SWIPE_PX = 60;

/** The capsule a pill opens out of and closes back into, in px. */
const PILL_SEED_PX = 36;

/**
 * A CSS `linear()` easing that follows a spring released from 0 and settling
 * on 1, overshooting a little first. `damping` below 1 sets the overshoot.
 */
function springEasing(damping: number, samples = 48): string {
  const stiffness = 10;
  const decay = damping * stiffness;
  const frequency = stiffness * Math.sqrt(1 - damping * damping);
  const points: string[] = [];
  for (let step = 0; step < samples; step++) {
    const t = step / samples;
    const value =
      1 -
      Math.exp(-decay * t) *
        (Math.cos(frequency * t) +
          (decay / frequency) * Math.sin(frequency * t));
    points.push(value.toFixed(4));
  }
  return `linear(${[...points, "1"].join(", ")})`;
}

const EASE_OUT = "cubic-bezier(0.22, 1, 0.36, 1)";
const EASE_IN = "cubic-bezier(0.4, 0, 1, 1)";
const SPRING = springEasing(0.7);
/** How long each kind of move takes. */
const SLIDE_MS = 450;
const MORPH_MS = 600;
const FADE_MS = 220;
const LEAVE_MS = 280;
/** Long enough for any of the exits above to finish. */
const EXIT_MS = 400;

interface ToasterProps {
  corner?: ToastCorner;
  motion?: ToastMotion;
  presentation?: ToastPresentation;
}

/** Where a toast sits in the stack. */
interface Spot {
  index: number;
  y: number;
  scale: number;
  /** Squeezed to the front toast's height while behind it, else its own. */
  height: number | undefined;
}

/** A dismissed toast, kept on screen while it leaves. */
interface Leaving {
  toast: Toast;
  spot: Spot;
  /** Thrown off by a swipe, rather than closed. */
  swiped: boolean;
}

interface Size {
  width: number;
  height: number;
}

function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  useLayoutEffect(() => {
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
  presentation = "card",
}: ToasterProps) {
  const { toasts, dismiss, hold } = useToaster();
  const reduced = useReducedMotion();
  const right = corner.endsWith("right");
  const bottom = corner.startsWith("bottom");
  /** Away from the corner's edge: up for the bottom corners. */
  const away = bottom ? -1 : 1;

  const [leaving, setLeaving] = useState<Leaving[]>([]);
  const [sizes, setSizes] = useState<Record<number, Size>>({});
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const expanded = hovered || focused;
  const shown = toasts.slice(0, VISIBLE);
  const empty = shown.length === 0 && leaving.length === 0;
  const listRef = useRef<HTMLOListElement | null>(null);

  // Radix portals each toast into the viewport, so React's own enter and
  // focus events (which follow the component tree) never reach it from a
  // toast; the DOM's do.
  useLayoutEffect(() => {
    const list = listRef.current;
    if (list === null) return;
    const enter = (): void => setHovered(true);
    const leave = (): void => setHovered(false);
    const focusIn = (): void => setFocused(true);
    const focusOut = (event: FocusEvent): void => {
      const next = event.relatedTarget;
      if (!(next instanceof Node && list.contains(next))) setFocused(false);
    };
    list.addEventListener("pointerenter", enter);
    list.addEventListener("pointerleave", leave);
    list.addEventListener("focusin", focusIn);
    list.addEventListener("focusout", focusOut);
    return () => {
      list.removeEventListener("pointerenter", enter);
      list.removeEventListener("pointerleave", leave);
      list.removeEventListener("focusin", focusIn);
      list.removeEventListener("focusout", focusOut);
    };
  }, []);

  // New toasts wait while the pointer is on the stack, so nothing shifts
  // under a click; keyboard focus does not hang on positions, so it gets
  // them at once. (Radix pauses the countdowns, also while the window is
  // away or focus is in the stack.)
  useLayoutEffect(() => {
    hold(hovered);
  }, [hovered, hold]);
  useLayoutEffect(() => () => hold(false), [hold]);

  // A stack that empties under the pointer never hears it leave.
  if (empty && (hovered || focused)) {
    setHovered(false);
    setFocused(false);
  }

  const heightOf = (toast: Toast | undefined): number =>
    toast ? (sizes[toast.id]?.height ?? 0) : 0;
  const front = heightOf(shown[0]);
  const offsets: number[] = [];
  let fanned = 0;
  for (const toast of shown) {
    offsets.push(fanned);
    fanned += heightOf(toast) + GAP_PX;
  }
  const fannedExtent = Math.max(0, fanned - GAP_PX);
  const stackedExtent = front + Math.max(0, shown.length - 1) * PEEK_PX;

  const spotOf = (toast: Toast, index: number): Spot => {
    if (expanded) {
      return {
        index,
        y: away * (offsets[index] ?? 0),
        scale: 1,
        height: undefined,
      };
    }
    if (index === 0) return { index, y: 0, scale: 1, height: undefined };
    // Behind, at most as tall as the front one and lined up on its far edge,
    // so exactly a peek of each shows past it.
    const own = heightOf(toast);
    const height = own > 0 && front > 0 ? Math.min(own, front) : undefined;
    const lift = height === undefined ? 0 : front - height;
    return {
      index,
      y: away * (index * PEEK_PX + lift),
      scale: 1 - index * SHRINK,
      height,
    };
  };

  // Radix has closed it (timer, swipe, Escape or its close button); it
  // stays on screen while it leaves, then goes from the queue.
  const close = (toast: Toast, swiped: boolean): void => {
    const index = shown.findIndex((candidate) => candidate.id === toast.id);
    if (index === -1) return;
    const spot = spotOf(toast, index);
    setLeaving((previous) =>
      previous.some((entry) => entry.toast.id === toast.id)
        ? previous
        : [...previous, { toast, spot, swiped }],
    );
    window.setTimeout(() => {
      setLeaving((previous) =>
        previous.filter((entry) => entry.toast.id !== toast.id),
      );
      setSizes(({ [toast.id]: _, ...rest }) => rest);
    }, EXIT_MS);
    dismiss(toast.id);
  };

  // Placed before paint, in its corner whatever page is under it. It is
  // anchored by that corner and grows away from it, so fanning out never
  // moves it.
  const [insets, setInsets] = useState<Insets | null>(null);
  useLayoutEffect(() => {
    if (empty) return;
    const next = placeToast(
      corner,
      Number.parseFloat(getComputedStyle(document.documentElement).fontSize),
    );
    setInsets((previous) =>
      previous?.x === next.x && previous.y === next.y ? previous : next,
    );
  }, [corner, empty]);
  /** How far the anchored edge is from the stack, to travel in from past it. */
  const edgeGap = insets?.y ?? 0;

  // One list in a fixed order, newest first, so a toast that starts leaving
  // stays the same element and its move carries on where it was.
  const items = [
    ...shown.map((toast, index) => ({
      toast,
      spot: spotOf(toast, index),
      leaving: null,
    })),
    ...leaving.map((entry) => ({ ...entry, leaving: entry })),
  ].sort((a, b) => b.toast.id - a.toast.id);

  return (
    <RadixToast.Provider
      swipeDirection={right ? "right" : "left"}
      swipeThreshold={SWIPE_PX}
    >
      {items.map(({ toast, spot, leaving: exit }) => (
        <ToastItem
          key={toast.id}
          toast={toast}
          pill={presentation === "pill"}
          spot={spot}
          exit={exit}
          bottom={bottom}
          right={right}
          edgeGap={edgeGap}
          expanded={expanded}
          slide={motion === "slide"}
          reduced={reduced}
          onSize={(size) =>
            setSizes((previous) =>
              previous[toast.id]?.height === size.height &&
              previous[toast.id]?.width === size.width
                ? previous
                : { ...previous, [toast.id]: size },
            )
          }
          onClose={(swiped) => close(toast, swiped)}
        />
      ))}
      <RadixToast.Viewport
        ref={listRef}
        style={{
          [right ? "right" : "left"]: insets?.x ?? `${EDGE_REM}rem`,
          [bottom ? "bottom" : "top"]:
            insets?.y ?? `${bottom ? EDGE_REM : TOP_REM}rem`,
          height: empty ? 0 : expanded ? fannedExtent : stackedExtent,
          width: TOAST_WIDTH,
        }}
        className="fixed z-40 m-0 list-none p-0 outline-none"
      />
    </RadixToast.Provider>
  );
}

interface ToastItemProps {
  toast: Toast;
  /** Shown as a capsule that opens out, rather than a card. */
  pill: boolean;
  spot: Spot;
  /** Set once it has been dismissed and is on its way out. */
  exit: Leaving | null;
  bottom: boolean;
  right: boolean;
  /** Distance from the stack to the anchored edge, in px. */
  edgeGap: number;
  expanded: boolean;
  slide: boolean;
  reduced: boolean;
  onSize: (size: Size) => void;
  onClose: (swiped: boolean) => void;
}

function ToastItem({
  toast,
  pill,
  spot,
  exit,
  bottom,
  right,
  edgeGap,
  expanded,
  slide,
  reduced,
  onSize,
  onClose,
}: ToastItemProps) {
  const behind = spot.index > 0 && !expanded;

  // Its own size, whatever the stack squeezes it to. Then, once the first
  // frame's style is in (read back to force it), it is let go to its place,
  // so the way in is a transition from where it starts: past the anchored
  // edge for a card, the seed capsule for a pill.
  const rootRef = useRef<HTMLLIElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const [natural, setNatural] = useState<Size | null>(null);
  const [entered, setEntered] = useState(false);
  const sizeRef = useRef(onSize);
  sizeRef.current = onSize;
  useLayoutEffect(() => {
    const root = rootRef.current;
    const content = contentRef.current;
    if (root === null || content === null) return;
    const measure = (): void => {
      const border = root.offsetHeight - root.clientHeight;
      const size = {
        width: content.offsetWidth + border,
        height: content.offsetHeight + border,
      };
      setNatural(size);
      sizeRef.current(size);
    };
    measure();
    void root.offsetWidth;
    setEntered(true);
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    return () => observer.disconnect();
  }, []);

  // Radix's swipe ends by closing it; remember that it was thrown, so it
  // carries on the way it went.
  const swiped = useRef(false);

  // Out of its place: on the way in, or leaving other than by a swipe.
  const outOfPlace = !entered || (exit !== null && !exit.swiped);
  const thrown = exit?.swiped && !reduced ? (right ? 1 : -1) : null;
  // A card past its anchored face: its own height plus the gap to the edge.
  const offEdge =
    outOfPlace && slide && !pill && !reduced
      ? ` + ${bottom ? 1 : -1} * (100% + ${edgeGap}px)`
      : "";
  // A pill shrinks back into its seed.
  const seeded = outOfPlace && pill && !reduced;
  const width = pill ? (seeded ? PILL_SEED_PX : natural?.width) : undefined;
  const height = pill
    ? seeded
      ? PILL_SEED_PX
      : (spot.height ?? natural?.height)
    : spot.height;
  const scale = spot.scale * (seeded ? 0.9 : 1);
  // Radix sets the drag offset while a swipe is under way.
  const x = thrown
    ? `${thrown * 120}%`
    : "var(--radix-toast-swipe-move-x, 0px)";

  const style: CSSProperties = {
    transform: `translateX(${x}) translateY(calc(${spot.y}px${offEdge})) scale(${scale})`,
    transformOrigin: pill
      ? `${right ? "right" : "left"} ${bottom ? "top" : "bottom"}`
      : `center ${bottom ? "top" : "bottom"}`,
    // Leaving by its edge, it passes behind the ones in front of it.
    zIndex: thrown ? VISIBLE + 1 : VISIBLE - spot.index,
    width,
    height,
    transition: reduced
      ? `opacity ${FADE_MS}ms ease-out`
      : exit && !thrown
        ? `transform ${LEAVE_MS}ms ${EASE_IN}, opacity ${LEAVE_MS}ms ${EASE_IN}, width ${LEAVE_MS}ms ${EASE_IN}, height ${LEAVE_MS}ms ${EASE_IN}`
        : pill
          ? `transform ${MORPH_MS}ms ${SPRING}, width ${MORPH_MS}ms ${SPRING}, height ${MORPH_MS}ms ${SPRING}, opacity ${FADE_MS}ms ease-out`
          : `transform ${SLIDE_MS}ms ${EASE_OUT}, height ${SLIDE_MS}ms ${EASE_OUT}, opacity ${FADE_MS}ms ease-out`,
  };
  if (exit || (!entered && !seeded)) style.opacity = 0;
  else if (behind) style.opacity = 1 - spot.index * 0.15;

  const content = (
    <div
      ref={contentRef}
      // `TOAST_WIDTH` less the Root's 1px border on each side, so the card
      // itself is exactly that wide.
      style={{ width: `calc(${TOAST_WIDTH} - 2px)` }}
      className="flex items-start gap-2 py-2 pr-1.5 pl-2"
    >
      <ToneGlyph tone={toast.tone} round={pill} />
      {/* Padded so its first line is centred on the glyph and close button
          (`size-5`) beside it, however many lines follow; up to 12 lines, then
          it scrolls. */}
      <RadixToast.Title className="max-h-[calc(11lh+1.25rem)] min-w-0 flex-1 overflow-y-auto font-medium text-[12.5px] text-fg-strong leading-snug [overflow-wrap:anywhere] py-[calc((1.25rem-1lh)/2)]">
        {toast.message}
      </RadixToast.Title>
      <RadixToast.Close
        aria-label="Dismiss notification"
        className={`grid size-5 shrink-0 cursor-pointer place-items-center text-fg-faint transition-colors hover:bg-surface-strong hover:text-fg focus-visible:outline-2 focus-visible:outline-accent ${pill ? "rounded-full" : "rounded-md"} sharper:rounded`}
      >
        <svg
          viewBox="0 0 24 24"
          className="size-3"
          fill="none"
          stroke="currentColor"
          strokeWidth={2.4}
          strokeLinecap="round"
          aria-hidden="true"
        >
          <path d="M7 7l10 10M17 7L7 17" />
        </svg>
      </RadixToast.Close>
    </div>
  );

  // Behind another, or still a seed, only the shape shows; the content
  // cross-fades in as it opens and out as it closes.
  const hidden = behind || seeded;

  return (
    <RadixToast.Root
      ref={rootRef}
      // Kept mounted after it closes, for the way out.
      forceMount
      open={exit === null}
      onOpenChange={(open) => {
        if (!open) onClose(swiped.current);
      }}
      duration={toast.duration}
      type={toast.tone === "error" ? "foreground" : "background"}
      onSwipeEnd={() => {
        swiped.current = true;
      }}
      // Reading, swiping or closing a notification never steals focus from
      // the page.
      onMouseDown={(event) => event.preventDefault()}
      inert={exit !== null}
      style={style}
      className={`absolute w-max ${right ? "right-0" : "left-0"} ${bottom ? "bottom-0" : "top-0"} overflow-hidden border border-line-strong bg-panel text-fg shadow-black/30 shadow-xl outline-accent focus-visible:outline-2 data-[swipe=move]:opacity-90! data-[swipe=move]:transition-none! ${
        pill
          ? `${right ? "right-0" : "left-0"} flex items-center justify-center rounded-full sharper:rounded-md`
          : "rounded-xl sharper:rounded-md"
      } ${exit ? "pointer-events-none" : ""}`}
    >
      {pill && (
        <span
          aria-hidden="true"
          className={`absolute inset-0 grid place-items-center transition-opacity duration-200 ${
            seeded ? "opacity-100" : "opacity-0"
          }`}
        >
          <ToneGlyph tone={toast.tone} round />
        </span>
      )}
      <div
        className={`transition-opacity ${
          hidden
            ? "opacity-0 duration-150"
            : "opacity-100 delay-100 duration-300"
        }`}
      >
        {content}
      </div>
    </RadixToast.Root>
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

function ToneGlyph({ tone, round }: { tone: ToastTone; round?: boolean }) {
  return (
    <span
      aria-hidden="true"
      className={`grid size-5 shrink-0 place-items-center ${round ? "rounded-full" : "rounded-md"} sharper:rounded ${GLYPH_STYLES[tone]}`}
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
