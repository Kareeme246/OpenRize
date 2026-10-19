import {
  type KeyboardEvent,
  type Ref,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { type Toast, type ToastTone, useToaster } from "../hooks/useToast";

/**
 * Where toasts appear: one card at a time, sliding in from a corner of the
 * main window. The default, bottom-right, keeps clear of the sidebar and its
 * tracking pill, of every page's header controls, and of the break panel and
 * its corner pill (separate windows at the top of the screen). It sits on
 * `z-40`, above page menus (`z-30`) and below pickers and tooltips; modal
 * sheets and dialogs are on the top layer, so a toast never covers one. A
 * side panel that must stay readable, like the entry review sheet, carries
 * `data-toast-avoid`, and the card steps beside it (or above, without room).
 *
 * It pauses while the pointer is on it, while focus is inside it, and while
 * the window is in the background, so it can't vanish mid-read or mid-click.
 * F8 moves focus to it, Escape there dismisses it, and focus goes back to
 * where it was; a toast never takes focus on its own. Screen readers hear it
 * through a polite status region, or an assertive alert for an error.
 */

export type ToastCorner =
  | "top-left"
  | "top-right"
  | "bottom-left"
  | "bottom-right";
/** `slide` moves in from the corner's side; `fade` only fades. */
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

/** Where a sliding card comes from and leaves to; reduced motion only fades. */
const SLIDE_LEFT = {
  enter: "motion-safe:starting:-translate-x-6",
  exit: "motion-safe:-translate-x-6",
};
const SLIDE_RIGHT = {
  enter: "motion-safe:starting:translate-x-6",
  exit: "motion-safe:translate-x-6",
};
const SLIDES: Record<ToastCorner, { enter: string; exit: string }> = {
  "top-left": SLIDE_LEFT,
  "top-right": SLIDE_RIGHT,
  "bottom-left": SLIDE_LEFT,
  "bottom-right": SLIDE_RIGHT,
};

/** How long a dismissed toast takes to leave, matching `duration-200`. */
const EXIT_MS = 200;
/** After a pause, at least this long remains, so leaving it never snaps it shut. */
const RESUME_MS = 1500;
const FOCUS_KEY = "F8";

interface ToasterProps {
  corner?: ToastCorner;
  motion?: ToastMotion;
}

export function Toaster({
  corner = "bottom-right",
  motion = "slide",
}: ToasterProps) {
  const { current, dismiss, hold } = useToaster();
  // The toast on screen, kept for its exit after `current` clears.
  const [shown, setShown] = useState<Toast | null>(null);
  const leaving = current === null && shown !== null;
  /** Where focus was before F8 moved it into the toast. */
  const returnFocus = useRef<HTMLElement | null>(null);
  const cardRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (current !== null) {
      setShown(current);
      return;
    }
    const timer = window.setTimeout(() => setShown(null), EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [current]);

  useEffect(() => {
    if (current === null) return;
    const onKeyDown = (event: globalThis.KeyboardEvent): void => {
      if (event.key !== FOCUS_KEY || cardRef.current === null) return;
      event.preventDefault();
      if (!cardRef.current.contains(document.activeElement)) {
        returnFocus.current =
          document.activeElement instanceof HTMLElement
            ? document.activeElement
            : null;
      }
      // The action first: it is what the keyboard came for.
      const card = cardRef.current;
      (
        card.querySelector<HTMLElement>("[data-toast-action]") ??
        card.querySelector<HTMLElement>("button")
      )?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [current]);

  const close = (id: number): void => {
    // Dismissing from the keyboard hands focus back instead of dropping it.
    const active = document.activeElement;
    if (active instanceof HTMLElement && cardRef.current?.contains(active)) {
      const target = returnFocus.current;
      if (target?.isConnected) target.focus();
      else active.blur();
    }
    returnFocus.current = null;
    dismiss(id);
  };

  const announced = current?.tone === "error" ? null : current;
  const alerted = current?.tone === "error" ? current : null;

  return (
    <>
      {shown && (
        <ToastCard
          key={shown.id}
          ref={cardRef}
          toast={shown}
          leaving={leaving}
          corner={corner}
          slide={motion === "slide" ? SLIDES[corner] : null}
          onHold={hold}
          onClose={() => close(shown.id)}
        />
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
  ref: Ref<HTMLElement>;
  toast: Toast;
  leaving: boolean;
  corner: ToastCorner;
  slide: { enter: string; exit: string } | null;
  onHold: (held: boolean) => void;
  onClose: () => void;
}

function ToastCard({
  ref,
  toast,
  leaving,
  corner,
  slide,
  onHold,
  onClose,
}: ToastCardProps) {
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [windowActive, setWindowActive] = useState(() => document.hasFocus());
  const remaining = useRef(toast.duration);
  const paused = hovered || focused || !windowActive;

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

  // A toast the viewer is on is not swapped out for a newer one.
  useEffect(() => {
    onHold(hovered || focused);
  }, [hovered, focused, onHold]);
  useEffect(() => () => onHold(false), [onHold]);

  // Count down only while unpaused, keeping what is left across pauses.
  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  useEffect(() => {
    if (leaving || paused) return;
    const started = performance.now();
    const wait = Math.max(remaining.current, RESUME_MS);
    remaining.current = wait;
    const timer = window.setTimeout(() => closeRef.current(), wait);
    return () => {
      window.clearTimeout(timer);
      remaining.current -= performance.now() - started;
    };
  }, [leaving, paused]);

  // Placed before paint, and again whenever the page under it changes, so it
  // steps aside as soon as a panel it must not cover opens.
  const sectionRef = useRef<HTMLElement | null>(null);
  const [insets, setInsets] = useState<Insets | null>(null);
  useLayoutEffect(() => {
    const card = sectionRef.current;
    if (card === null) return;
    const place = (): void => {
      const avoid = [
        ...document.querySelectorAll<HTMLElement>("[data-toast-avoid]"),
      ]
        .map((element) => element.getBoundingClientRect())
        .filter((rect) => rect.width > 0 && rect.height > 0);
      const next = placeToast(
        corner,
        card.offsetWidth,
        card.offsetHeight,
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
  }, [corner]);
  const right = corner.endsWith("right");
  const bottom = corner.startsWith("bottom");

  const onKeyDown = (event: KeyboardEvent): void => {
    if (event.key !== "Escape") return;
    // Only this toast: not a page's own Escape (review mode, a menu) as well.
    event.preventDefault();
    event.stopPropagation();
    onClose();
  };

  return (
    <section
      ref={(element) => {
        sectionRef.current = element;
        if (typeof ref === "function") ref(element);
        else if (ref) ref.current = element;
      }}
      aria-label="Notification"
      inert={leaving}
      onPointerEnter={() => setHovered(true)}
      onPointerLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) {
          setFocused(false);
        }
      }}
      onKeyDown={onKeyDown}
      style={{
        [right ? "right" : "left"]: insets?.x ?? `${EDGE_REM}rem`,
        [bottom ? "bottom" : "top"]:
          insets?.y ?? `${bottom ? EDGE_REM : TOP_REM}rem`,
      }}
      className={`fixed z-40 w-[22rem] max-w-[calc(100vw-2rem)] rounded-xl border border-line-strong bg-panel p-3 text-fg shadow-black/30 shadow-xl transition-[opacity,translate,left,right,top,bottom] duration-200 ease-out starting:opacity-0 sharper:rounded-md ${
        slide?.enter ?? ""
      } ${leaving ? `opacity-0 ${slide?.exit ?? ""}` : ""}`}
    >
      <div className="flex items-start gap-2.5">
        <ToneGlyph tone={toast.tone} />
        <p className="min-w-0 flex-1 py-0.5 font-medium text-[12.5px] text-fg-strong leading-snug [overflow-wrap:anywhere]">
          {toast.message}
        </p>
        <button
          type="button"
          onClick={onClose}
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
              onClose();
            }}
            className="rounded-md bg-accent px-2.5 py-1 font-semibold text-[12px] text-accent-fg outline-accent outline-offset-2 transition-opacity hover:opacity-90 focus-visible:outline-2 sharper:rounded"
          >
            {toast.action.label}
          </button>
        </div>
      )}
    </section>
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
