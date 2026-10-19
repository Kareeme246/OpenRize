import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";

/**
 * The app's one way to confirm a short-lived result ("Undid split", "Exported
 * 12 entries"). Any component raises one with `useToast()`; `<Toaster />`
 * shows it. Errors that need fixing stay inline (InlineError, a
 * Sheet's error row); a toast only reports something that already happened.
 */

/** `neutral` for plain facts, `success` for a finished job, `error` for a failed one. */
export type ToastTone = "neutral" | "success" | "error";

export interface ToastAction {
  label: string;
  /** Runs on press; the toast closes after it. */
  onAction: () => void;
}

export interface ToastOptions {
  message: string;
  tone?: ToastTone;
  /** At most one follow-up, e.g. "Redo" or "Show in Finder". */
  action?: ToastAction;
  /** Milliseconds on screen while not paused. Defaults depend on the toast. */
  duration?: number;
  /**
   * Toasts about one ordered history, like entry undo: a newer toast in the
   * lane takes the action off older ones, which could no longer run in order.
   */
  lane?: string;
}

export interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
  action?: ToastAction;
  duration: number;
  lane?: string;
}

const BASE_MS = 4000;
/**
 * How long an action stays on offer, the same for every toast so Undo always
 * means the same window: once the toast is gone, the change stands.
 */
export const ACTION_MS = 8000;
/** A failure is worth a second read. */
const ERROR_MS = 8000;

function defaultDuration({ tone, action }: ToastOptions): number {
  if (tone === "error") return ERROR_MS;
  return action ? ACTION_MS : BASE_MS;
}

export type ShowToast = (options: ToastOptions) => void;

interface ToastState {
  /** Newest first. `<Toaster />` shows the first few; the rest wait their turn. */
  stack: Toast[];
  /**
   * Raised while the stack was held under the pointer or focus, newest first.
   * They join the front once it is let go, so nothing shifts under a click.
   */
  pending: Toast[];
}

export interface ToasterApi {
  /** Newest first. */
  toasts: Toast[];
  dismiss: (id: number) => void;
  /** The viewer is reading or reaching into the stack: don't add to it. */
  hold: (held: boolean) => void;
}

const ShowContext = createContext<ShowToast>(() => undefined);
const ToasterContext = createContext<ToasterApi>({
  toasts: [],
  dismiss: () => undefined,
  hold: () => undefined,
});

/** `toasts` without the action of any in `lane`. */
function retire(toasts: Toast[], lane: string | undefined): Toast[] {
  if (lane === undefined) return toasts;
  return toasts.map((toast) =>
    toast.lane === lane && toast.action
      ? { ...toast, action: undefined }
      : toast,
  );
}

/** Raises a toast from anywhere under `ToastProvider`. Stable across renders. */
export function useToast(): ShowToast {
  return useContext(ShowContext);
}

/** For `<Toaster />` only. */
export function useToaster(): ToasterApi {
  return useContext(ToasterContext);
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<ToastState>({ stack: [], pending: [] });
  const nextId = useRef(1);
  const held = useRef(false);

  const show = useCallback<ShowToast>((options) => {
    const toast: Toast = {
      id: nextId.current++,
      message: options.message,
      tone: options.tone ?? "neutral",
      action: options.action,
      duration: options.duration ?? defaultDuration(options),
      lane: options.lane,
    };
    setState((previous) => {
      const stack = retire(previous.stack, toast.lane);
      const pending = retire(previous.pending, toast.lane);
      return held.current && stack.length > 0
        ? { stack, pending: [toast, ...pending] }
        : { stack: [toast, ...pending, ...stack], pending: [] };
    });
  }, []);

  const dismiss = useCallback((id: number): void => {
    setState((previous) => ({
      stack: previous.stack.filter((toast) => toast.id !== id),
      pending: previous.pending.filter((toast) => toast.id !== id),
    }));
  }, []);

  const hold = useCallback((next: boolean): void => {
    held.current = next;
    if (next) return;
    setState((previous) =>
      previous.pending.length > 0
        ? { stack: [...previous.pending, ...previous.stack], pending: [] }
        : previous,
    );
  }, []);

  const toaster = useMemo<ToasterApi>(
    () => ({ toasts: state.stack, dismiss, hold }),
    [state.stack, dismiss, hold],
  );

  return (
    <ShowContext.Provider value={show}>
      <ToasterContext.Provider value={toaster}>
        {children}
      </ToasterContext.Provider>
    </ShowContext.Provider>
  );
}
