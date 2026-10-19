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
}

export interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
  action?: ToastAction;
  duration: number;
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
  /** What the toast shows. */
  current: Toast | null;
  /**
   * The newest toast raised while `current` was held under the pointer or
   * focus. Only one waits: anything newer replaces it, so toasts never pile up.
   */
  next: Toast | null;
}

export interface ToasterApi {
  current: Toast | null;
  dismiss: (id: number) => void;
  /** The viewer is reading or reaching for `current`: don't swap it out. */
  hold: (held: boolean) => void;
}

const ShowContext = createContext<ShowToast>(() => undefined);
const ToasterContext = createContext<ToasterApi>({
  current: null,
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
  const [state, setState] = useState<ToastState>({ current: null, next: null });
  const nextId = useRef(1);
  const held = useRef(false);

  const show = useCallback<ShowToast>((options) => {
    const toast: Toast = {
      id: nextId.current++,
      message: options.message,
      tone: options.tone ?? "neutral",
      action: options.action,
      duration: options.duration ?? defaultDuration(options),
    };
    // A new result replaces the old one, unless the viewer is on it right now.
    setState((previous) =>
      held.current && previous.current
        ? { ...previous, next: toast }
        : { current: toast, next: null },
    );
  }, []);

  const dismiss = useCallback((id: number): void => {
    setState((previous) =>
      previous.current?.id === id
        ? { current: previous.next, next: null }
        : previous,
    );
  }, []);

  const hold = useCallback((next: boolean): void => {
    held.current = next;
    if (next) return;
    setState((previous) =>
      previous.next ? { current: previous.next, next: null } : previous,
    );
  }, []);

  const toaster = useMemo<ToasterApi>(
    () => ({ current: state.current, dismiss, hold }),
    [state.current, dismiss, hold],
  );

  return (
    <ShowContext.Provider value={show}>
      <ToasterContext.Provider value={toaster}>
        {children}
      </ToasterContext.Provider>
    </ShowContext.Provider>
  );
}
