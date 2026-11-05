import * as RadixToast from "@radix-ui/react-toast";
import { type CSSProperties, useLayoutEffect, useRef, useState } from "react";
import { type Toast, type ToastTone, useToaster } from "../hooks/useToast";

/**
 * Toast stack layout and motion; timers, swipe, focus and announcements are
 * Radix Toast's. Sits on z-40: above page menus (z-30), below pickers and
 * tooltips.
 */

export type ToastCorner =
  | "top-left"
  | "top-right"
  | "bottom-left"
  | "bottom-right";
export type ToastMotion = "slide" | "fade";
export type ToastPresentation = "card" | "pill";

const EDGE_REM = 1;
const TOAST_WIDTH = "min(16rem, calc(100vw - 2rem))";
const TOP_REM = 3.5;

interface Insets {
  x: number;
  y: number;
}

export function placeToast(corner: ToastCorner, rem: number): Insets {
  return {
    x: EDGE_REM * rem,
    y: (corner.startsWith("bottom") ? EDGE_REM : TOP_REM) * rem,
  };
}

const VISIBLE = 3;
const PEEK_PX = 14;
const SHRINK = 0.05;
const GAP_PX = 10;
const SWIPE_PX = 60;

const PILL_SEED_PX = 36;

/** CSS `linear()` easing following a spring; `damping` below 1 overshoots. */
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

interface Spot {
  index: number;
  y: number;
  scale: number;
  height: number | undefined;
}

interface Leaving {
  toast: Toast;
  spot: Spot;
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
  const away = bottom ? -1 : 1;

  const [leaving, setLeaving] = useState<Leaving[]>([]);
  const [sizes, setSizes] = useState<Record<number, Size>>({});
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const expanded = hovered || focused;
  const shown = toasts.slice(0, VISIBLE);
  const empty = shown.length === 0 && leaving.length === 0;
  const listRef = useRef<HTMLOListElement | null>(null);

  // Radix portals toasts into the viewport, so React's enter/focus events never
  // reach it; the DOM's do.
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

  // New toasts wait while the pointer is on the stack so nothing shifts under a
  // click; keyboard focus gets them at once.
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

  // Placed before paint and anchored by its corner, so fanning out never moves
  // it.
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
  pill: boolean;
  spot: Spot;
  exit: Leaving | null;
  bottom: boolean;
  right: boolean;
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

  // Starts at its own size; once the first frame's style is read back, it is
  // released to its place so the way in is a transition.
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

  const outOfPlace = !entered || (exit !== null && !exit.swiped);
  const thrown = exit?.swiped && !reduced ? (right ? 1 : -1) : null;
  const offEdge =
    outOfPlace && slide && !pill && !reduced
      ? ` + ${bottom ? 1 : -1} * (100% + ${edgeGap}px)`
      : "";
  const seeded = outOfPlace && pill && !reduced;
  const width = pill ? (seeded ? PILL_SEED_PX : natural?.width) : undefined;
  const height = pill
    ? seeded
      ? PILL_SEED_PX
      : (spot.height ?? natural?.height)
    : spot.height;
  const scale = spot.scale * (seeded ? 0.9 : 1);
  const x = thrown
    ? `${thrown * 120}%`
    : "var(--radix-toast-swipe-move-x, 0px)";

  const style: CSSProperties = {
    transform: `translateX(${x}) translateY(calc(${spot.y}px${offEdge})) scale(${scale})`,
    transformOrigin: pill
      ? `${right ? "right" : "left"} ${bottom ? "top" : "bottom"}`
      : `center ${bottom ? "top" : "bottom"}`,
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

  const hidden = behind || seeded;

  return (
    <RadixToast.Root
      ref={rootRef}
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
