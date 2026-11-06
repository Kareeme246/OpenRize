import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";

const RELEASE_ATTEMPTS = 20;
const RELEASE_RETRY_MS = 50;

/**
 * Unlistens, retrying while Tauri wires the listener up: unlistening before the
 * webview-side eval lands throws before the backend is told, leaving it
 * registered for good.
 */
function release(unlisten: () => void, attempt = 0): void {
  Promise.resolve()
    .then(() => unlisten())
    .catch(() => {
      if (attempt < RELEASE_ATTEMPTS) {
        window.setTimeout(
          () => release(unlisten, attempt + 1),
          RELEASE_RETRY_MS,
        );
      }
    });
}

/**
 * Subscribes to a Rust event for the component's lifetime and calls the latest
 * `handler`; a ref avoids stale listeners overwriting fresher state.
 */
export function useTauriEvent<T>(
  event: string,
  handler: (payload: T) => void,
): void {
  const latest = useRef(handler);
  latest.current = handler;

  useEffect(() => {
    let stopped = false;
    let stop: (() => void) | undefined;
    listen<T>(event, (message) => {
      if (!stopped) latest.current(message.payload);
    })
      .then((unlisten) => {
        if (stopped) release(unlisten);
        else stop = unlisten;
      })
      .catch((cause: unknown) => {
        console.error(`Could not listen to ${event}`, cause);
      });
    return () => {
      stopped = true;
      if (stop) release(stop);
    };
  }, [event]);
}
