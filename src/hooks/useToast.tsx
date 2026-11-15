import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
} from "react";

export type ToastTone = "neutral" | "success" | "error";

export interface ToastOptions {
  message: string;
  tone?: ToastTone;
  duration?: number;
}

export interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
  duration: number;
}

const BASE_MS = 3000;
const ERROR_MS = 7000;

export type ShowToast = (options: ToastOptions) => void;

interface ToastState {
  stack: Toast[];
  /**
   * Raised while the stack was held under the pointer or focus; they join the
   * front once it is let go.
   */
  pending: Toast[];
}

export interface ToasterApi {
  toasts: Toast[];
  dismiss: (id: number) => void;
  hold: (held: boolean) => void;
}

const ShowContext = createContext<ShowToast>(() => undefined);
const ToasterContext = createContext<ToasterApi>({
  toasts: [],
  dismiss: () => undefined,
  hold: () => undefined,
});

export function useToast(): ShowToast {
  return useContext(ShowContext);
}

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
