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
 * The app's one way to confirm a short-lived result ("Entry deleted",
 * "Exported 12 entries"). Any component raises one with `useToast()`;
 * `<Toaster />` shows it. A toast only informs: it carries no action buttons
 * (only a control to close it), and errors that need fixing stay inline
 * (InlineError, a Sheet's error row).
 */

/** `neutral` for plain facts, `success` for a finished job, `error` for a failed one. */
export type ToastTone = "neutral" | "success" | "error";

export interface ToastOptions {
  message: string;
  tone?: ToastTone;
  /** Milliseconds on screen while not paused. Defaults depend on the tone. */
  duration?: number;
}

export interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
  duration: number;
}

/** How long a toast stays while not paused. */
const BASE_MS = 3000;
/** A failure is worth a second read. */
const ERROR_MS = 7000;

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
  /** The pointer is on the stack: don't add to it. */
  hold: (held: boolean) => void;
}

const ShowContext = createContext<ShowToast>(() => undefined);
const ToasterContext = createContext<ToasterApi>({
  toasts: [],
  dismiss: () => undefined,
  hold: () => undefined,
});

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
      duration:
        options.duration ?? (options.tone === "error" ? ERROR_MS : BASE_MS),
    };
    setState((previous) =>
      held.current && previous.stack.length > 0
        ? { ...previous, pending: [toast, ...previous.pending] }
        : {
            stack: [toast, ...previous.pending, ...previous.stack],
            pending: [],
          },
    );
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
