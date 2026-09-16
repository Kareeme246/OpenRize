import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";

/**
 * The one "not built yet" modal every new stub opens. Kept global so pages,
 * cards, and the top bar all trigger the identical dialog without each one
 * owning markup. Only new scaffolding should use this — retrofitting it over
 * existing finished controls is out of scope.
 */
interface NotImplementedApi {
  show: (feature: string) => void;
}

const NotImplementedContext = createContext<NotImplementedApi>({
  show: () => undefined,
});

export function useNotImplemented(): NotImplementedApi {
  return useContext(NotImplementedContext);
}

export function NotImplementedProvider({ children }: { children: ReactNode }) {
  const [feature, setFeature] = useState<string | null>(null);
  const show = useCallback((next: string) => setFeature(next), []);

  return (
    <NotImplementedContext.Provider value={{ show }}>
      {children}
      {feature !== null && (
        <NotImplementedDialog
          feature={feature}
          onClose={() => setFeature(null)}
        />
      )}
    </NotImplementedContext.Provider>
  );
}

function NotImplementedDialog({
  feature,
  onClose,
}: {
  feature: string;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) return;
    dialog.showModal();
    dialog.focus();
    return () => dialog.close();
  }, []);

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop click-to-cancel; native onCancel (Escape) already provides the keyboard equivalent
    <dialog
      ref={ref}
      tabIndex={-1}
      onCancel={onClose}
      onClick={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
      className="m-auto rounded-2xl bg-transparent p-0 text-white outline-none backdrop:bg-black/65"
    >
      <div className="w-[340px] max-w-[calc(100vw-2rem)] rounded-2xl border border-white/10 bg-ink-900 p-4 shadow-2xl">
        <div className="flex items-center gap-2">
          <span className="rounded-full border border-white/10 bg-white/5 px-2 py-0.5 font-mono text-[9.5px] uppercase tracking-wider text-white/40">
            Not implemented
          </span>
        </div>
        <h2 className="mt-2.5 text-[13.5px] font-semibold">{feature}</h2>
        <p className="mt-1.5 text-[12.5px] leading-relaxed text-white/50">
          This isn&rsquo;t built yet. It&rsquo;s a stub for a later pass, so
          nothing happened when you clicked.
        </p>
        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3 py-1.5 text-[12.5px] font-semibold text-[#04160c]"
          >
            Got it
          </button>
        </div>
      </div>
    </dialog>
  );
}
