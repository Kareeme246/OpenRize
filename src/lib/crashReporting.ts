import * as api from "./api";
import type { ClientCrash } from "./types";

/**
 * Hands UI failures to Rust, which decides what a report may hold (see
 * `src-tauri/core/src/crashes.rs`). Everything passed here is treated as
 * untrusted free text on the Rust side: messages and stacks can quote the
 * user's data, so only a fixed description and anonymous frames survive, and
 * nothing is sent anywhere unless the user turned crash reporting on.
 */

/** A page that throws in a loop must not flood the backend. */
const MAX_REPORTS_PER_PAGE = 10;

/** Browser noise that is not a failure of the app. */
const BENIGN_MESSAGE = /^ResizeObserver loop/;

let reported = 0;

export function reportClientCrash(crash: ClientCrash): void {
  if (reported >= MAX_REPORTS_PER_PAGE) return;
  reported += 1;
  api.recordClientCrash(crash).catch(() => undefined);
}

/** The parts of a thrown value worth passing on: only an `Error` has any. */
export function describeError(
  value: unknown,
): Pick<ClientCrash, "name" | "message" | "stack"> {
  return value instanceof Error
    ? { name: value.name, message: value.message, stack: value.stack }
    : {};
}

/** Records uncaught errors and unhandled promise rejections in this window. */
export function installCrashHandlers(): void {
  window.addEventListener("error", (event) => {
    if (BENIGN_MESSAGE.test(event.message)) return;
    reportClientCrash({
      source: "frontend_error",
      ...describeError(event.error),
      message: event.message,
    });
  });

  window.addEventListener("unhandledrejection", (event) => {
    reportClientCrash({
      source: "unhandled_rejection",
      ...describeError(event.reason),
    });
  });
}
