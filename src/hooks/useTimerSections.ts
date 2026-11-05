import { useCallback, useEffect, useState } from "react";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import type { TimerSection } from "../lib/timers";
import { useTauriEvent } from "./useTauriEvent";
import { useToast } from "./useToast";

export interface TimerSectionsApi {
  sections: TimerSection[];
  error: string | null;
  /** Adds a section after the others, moving `timerId` into it if given. */
  create: (name: string, timerId?: string) => void;
  rename: (id: string, name: string) => void;
  /** Deletes a section; Rust keeps its stopwatches, in no section. */
  remove: (id: string) => void;
  reorder: (ids: string[]) => void;
}

let cachedSections: TimerSection[] | null = null;

/**
 * The Stopwatches page's sections. Rust owns them as it owns the timers:
 * every command answers with the full list, and a change made elsewhere
 * (`rize`) arrives as an event, so state is replaced, never patched.
 */
export function useTimerSections(): TimerSectionsApi {
  const [sections, setSectionsState] = useState<TimerSection[]>(
    () => cachedSections ?? [],
  );
  const [error, setError] = useState<string | null>(null);

  const setSections = useCallback((next: TimerSection[]) => {
    cachedSections = next;
    setSectionsState(next);
  }, []);

  useEffect(() => {
    api
      .listTimerSections()
      .then(setSections)
      .catch((cause: unknown) => setError(describeError(cause)));
  }, [setSections]);

  useTauriEvent<TimerSection[]>(api.TIMER_SECTIONS_CHANGED, (payload) => {
    setSections(payload);
    setError(null);
  });

  const showToast = useToast();
  const send = useCallback(
    (action: Promise<TimerSection[]>, message?: string): void => {
      action
        .then((next) => {
          setSections(next);
          setError(null);
          if (message) showToast({ message, tone: "success" });
        })
        .catch((cause: unknown) => setError(describeError(cause)));
    },
    [setSections, showToast],
  );

  const create = useCallback(
    (name: string, timerId?: string) =>
      send(
        api.createTimerSection(name, timerId),
        `Added section "${name.trim()}"`,
      ),
    [send],
  );
  const rename = useCallback(
    (id: string, name: string) =>
      send(api.renameTimerSection(id, name), `Renamed to "${name.trim()}"`),
    [send],
  );
  const remove = useCallback(
    (id: string) => {
      const name = sections.find((section) => section.id === id)?.name;
      send(
        api.deleteTimerSection(id),
        name === undefined ? undefined : `Deleted section "${name}"`,
      );
    },
    [send, sections],
  );
  const reorder = useCallback(
    (ids: string[]) => send(api.reorderTimerSections(ids)),
    [send],
  );

  return { sections, error, create, rename, remove, reorder };
}
