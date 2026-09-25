import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import {
  applyAppearance,
  DEFAULT_SETTINGS,
  type Settings,
  type StoragePaths,
} from "../lib/settings";
import { useTauriEvent } from "./useTauriEvent";

export interface SettingsApi {
  /** Always defined: defaults render until Rust answers, then are replaced. */
  settings: Settings;
  storage: StoragePaths | null;
  error: string | null;
  /** Merges a patch, persists it, and repaints immediately. */
  update: (patch: Partial<Settings>) => void;
}

const SettingsContext = createContext<SettingsApi>({
  settings: DEFAULT_SETTINGS,
  storage: null,
  error: null,
  update: () => undefined,
});

export function useSettings(): SettingsApi {
  return useContext(SettingsContext);
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [storage, setStorage] = useState<StoragePaths | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Latest value for `update`, so the callback stays stable without stale
  // reads - the same pattern useTimers uses for its send().
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  useEffect(() => {
    api
      .getSettings()
      .then((loaded) => {
        setSettings(loaded);
        applyAppearance(loaded);
      })
      .catch((cause: unknown) => setError(describeError(cause)));
    api
      .storagePaths()
      .then(setStorage)
      .catch((cause: unknown) => setError(describeError(cause)));
  }, []);

  // Rust owns preferences; adopt whatever it broadcasts (e.g. after a future
  // tray or CLI toggle).
  useTauriEvent<Settings>(api.SETTINGS_CHANGED, (payload) => {
    setSettings(payload);
    applyAppearance(payload);
    setError(null);
  });

  // "Follow system" has to keep following: re-resolve when the OS flips while
  // the app is open.
  useEffect(() => {
    if (settings.theme !== "system") return;
    const media = window.matchMedia("(prefers-color-scheme: light)");
    const onChange = (): void => applyAppearance(settings);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [settings]);

  const update = useCallback((patch: Partial<Settings>): void => {
    const next = { ...settingsRef.current, ...patch };
    // Paint first so the control feels instant; Rust confirms with the same
    // values (it only clamps retention).
    setSettings(next);
    applyAppearance(next);
    api
      .updateSettings(next)
      .then((saved) => {
        setSettings(saved);
        applyAppearance(saved);
        setError(null);
      })
      .catch((cause: unknown) => setError(describeError(cause)));
  }, []);

  return (
    <SettingsContext.Provider value={{ settings, storage, error, update }}>
      {children}
    </SettingsContext.Provider>
  );
}
