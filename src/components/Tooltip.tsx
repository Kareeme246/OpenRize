import {
  cloneElement,
  type FocusEvent,
  type PointerEvent,
  type ReactElement,
  type ReactNode,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";

/**
 * Hover tooltip replacing the native `title` bubble. Escape is swallowed while
 * one is open. Rendered in a top-layer portal (inside the open dialog, if any)
 * so clipping and modal inertness never hide it.
 */

export type TooltipPlacement = "top" | "bottom" | "left" | "right";

const SHOW_DELAY_MS = 450;
const WARM_WINDOW_MS = 300;
const GAP = 6;
const EDGE_MARGIN = 8;

const OPPOSITE: Record<TooltipPlacement, TooltipPlacement> = {
  top: "bottom",
  bottom: "top",
  left: "right",
  right: "left",
};

let closeActive: (() => void) | null = null;
let lastClosedAt = 0;

interface Position {
  left: number;
  top: number;
}

function fitsOn(
  side: TooltipPlacement,
  anchor: DOMRect,
  width: number,
  height: number,
  viewportWidth: number,
  viewportHeight: number,
): boolean {
  switch (side) {
    case "top":
      return anchor.top - GAP - height >= EDGE_MARGIN;
    case "bottom":
      return anchor.bottom + GAP + height <= viewportHeight - EDGE_MARGIN;
    case "left":
      return anchor.left - GAP - width >= EDGE_MARGIN;
    case "right":
      return anchor.right + GAP + width <= viewportWidth - EDGE_MARGIN;
  }
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(value, Math.max(min, max)));
}

export function placeTooltip(
  anchor: DOMRect,
  width: number,
  height: number,
  preferred: TooltipPlacement,
  viewportWidth: number,
  viewportHeight: number,
): Position {
  const candidates: TooltipPlacement[] = [
    preferred,
    OPPOSITE[preferred],
    ...(preferred === "top" || preferred === "bottom"
      ? (["right", "left"] as const)
      : (["bottom", "top"] as const)),
  ];
  const side =
    candidates.find((candidate) =>
      fitsOn(candidate, anchor, width, height, viewportWidth, viewportHeight),
    ) ?? preferred;

  const centreX = anchor.left + anchor.width / 2 - width / 2;
  const centreY = anchor.top + anchor.height / 2 - height / 2;
  const maxLeft = viewportWidth - EDGE_MARGIN - width;
  const maxTop = viewportHeight - EDGE_MARGIN - height;

  switch (side) {
    case "top":
      return {
        left: clamp(centreX, EDGE_MARGIN, maxLeft),
        top: clamp(anchor.top - GAP - height, EDGE_MARGIN, maxTop),
      };
    case "bottom":
      return {
        left: clamp(centreX, EDGE_MARGIN, maxLeft),
        top: clamp(anchor.bottom + GAP, EDGE_MARGIN, maxTop),
      };
    case "left":
      return {
        left: clamp(anchor.left - GAP - width, EDGE_MARGIN, maxLeft),
        top: clamp(centreY, EDGE_MARGIN, maxTop),
      };
    case "right":
      return {
        left: clamp(anchor.right + GAP, EDGE_MARGIN, maxLeft),
        top: clamp(centreY, EDGE_MARGIN, maxTop),
      };
  }
}

interface TriggerProps {
  "aria-label"?: string;
  "aria-describedby"?: string;
  onPointerEnter?: (event: PointerEvent<Element>) => void;
  onPointerLeave?: (event: PointerEvent<Element>) => void;
  onPointerDown?: (event: PointerEvent<Element>) => void;
  onFocus?: (event: FocusEvent<Element>) => void;
  onBlur?: (event: FocusEvent<Element>) => void;
}

interface TooltipProps {
  content: ReactNode;
  placement?: TooltipPlacement;
  /** Wraps the trigger in a `<span>` with these classes: for a disabled
   * control (no pointer events) or a child that cannot take event props. */
  wrapperClassName?: string;
  children: ReactElement<TriggerProps>;
}

export function Tooltip({
  content,
  placement = "top",
  wrapperClassName,
  children,
}: TooltipProps) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<Position | null>(null);
  const anchorRef = useRef<Element | null>(null);
  const bubbleRef = useRef<HTMLDivElement | null>(null);
  const timerRef = useRef<number | undefined>(undefined);
  const openRef = useRef(false);
  const hasContent =
    content !== undefined && content !== null && content !== "";

  const close = useCallback(() => {
    window.clearTimeout(timerRef.current);
    if (openRef.current) lastClosedAt = performance.now();
    openRef.current = false;
    setOpen(false);
    setPosition(null);
    if (closeActive === close) closeActive = null;
  }, []);

  const show = useCallback(
    (anchor: Element, immediately: boolean) => {
      anchorRef.current = anchor;
      window.clearTimeout(timerRef.current);
      const reveal = (): void => {
        if (closeActive && closeActive !== close) closeActive();
        closeActive = close;
        openRef.current = true;
        setOpen(true);
      };
      const warm = performance.now() - lastClosedAt < WARM_WINDOW_MS;
      if (immediately || warm) reveal();
      else timerRef.current = window.setTimeout(reveal, SHOW_DELAY_MS);
    },
    [close],
  );

  useEffect(() => () => close(), [close]);

  // Measure before paint so the bubble never flashes at the wrong spot. `content`
  // is a dependency only to re-measure when the text changes under an open bubble.
  // biome-ignore lint/correctness/useExhaustiveDependencies: see above
  useLayoutEffect(() => {
    if (!open) return;
    const anchor = anchorRef.current;
    const bubble = bubbleRef.current;
    if (!anchor || !bubble) return;
    // The popover top layer is only available on current WebKit.
    if (
      typeof bubble.showPopover === "function" &&
      !bubble.matches(":popover-open")
    ) {
      bubble.showPopover();
    }
    const { width, height } = bubble.getBoundingClientRect();
    setPosition(
      placeTooltip(
        anchor.getBoundingClientRect(),
        width,
        height,
        placement,
        document.documentElement.clientWidth,
        document.documentElement.clientHeight,
      ),
    );
  }, [open, content, placement]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent): void => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    // Capture, so Escape reaches us before a dialog or panel handler does.
    document.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("scroll", close, { capture: true, passive: true });
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("scroll", close, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  }, [open, close]);

  if (!hasContent) return children;

  const wrapped = wrapperClassName !== undefined;
  // In wrapper mode the child keeps its own handlers; only a cloned child needs
  // ours chained after them.
  const own: TriggerProps = wrapped ? {} : children.props;
  const handlers = {
    onPointerEnter: (event: PointerEvent<Element>) => {
      own.onPointerEnter?.(event);
      if (event.pointerType === "touch") return;
      show(event.currentTarget, false);
    },
    onPointerLeave: (event: PointerEvent<Element>) => {
      own.onPointerLeave?.(event);
      close();
    },
    onPointerDown: (event: PointerEvent<Element>) => {
      own.onPointerDown?.(event);
      close();
    },
    onFocus: (event: FocusEvent<Element>) => {
      own.onFocus?.(event);
      // Keyboard focus only (clicking a button also focuses it), and a cloned
      // trigger ignores focus bubbling up from a control nested inside it.
      if (!wrapped && event.target !== event.currentTarget) return;
      if (
        event.target instanceof Element &&
        event.target.matches(":focus-visible")
      ) {
        show(event.currentTarget, true);
      }
    },
    onBlur: (event: FocusEvent<Element>) => {
      own.onBlur?.(event);
      close();
    },
  };

  const repeatsName =
    typeof content === "string" && children.props["aria-label"] === content;
  const describedBy = open && !repeatsName ? id : undefined;

  const bubble =
    open &&
    createPortal(
      <div
        ref={bubbleRef}
        id={id}
        role="tooltip"
        popover="manual"
        style={{
          inset: "auto",
          left: position?.left ?? 0,
          top: position?.top ?? 0,
          visibility: position ? "visible" : "hidden",
          maxWidth: `min(16rem, calc(100vw - ${EDGE_MARGIN * 2}px))`,
        }}
        className="pointer-events-none fixed z-[100] m-0 w-max select-none overflow-visible rounded-md border border-line-strong bg-panel px-2 py-1 text-left font-medium text-[11.5px] text-fg leading-snug shadow-lg"
      >
        {content}
      </div>,
      anchorRef.current?.closest("dialog") ?? document.body,
    );

  if (wrapped) {
    return (
      <>
        <span
          {...handlers}
          aria-describedby={describedBy}
          className={`${wrapperClassName} [&>:disabled]:pointer-events-none`}
        >
          {children}
        </span>
        {bubble}
      </>
    );
  }

  return (
    <>
      {cloneElement(children, {
        ...handlers,
        "aria-describedby": describedBy ?? children.props["aria-describedby"],
      })}
      {bubble}
    </>
  );
}
