import { useCallback, useState } from "react";
import * as api from "../lib/api";
import { relaunchAfter } from "../lib/relaunch";

/**
 * The one way the UI restarts the app for a permission: the relaunch screen
 * and the first-run Restart button both use it. `prepare` runs first, and its
 * failure does not stop the relaunch.
 */
export function useRelaunch(prepare?: () => Promise<void>) {
  const [relaunching, setRelaunching] = useState(false);
  const [failed, setFailed] = useState(false);

  const relaunch = useCallback((): void => {
    setRelaunching(true);
    setFailed(false);
    relaunchAfter(prepare, api.relaunchApp, (cause) =>
      console.error("Could not prepare to relaunch", api.describeError(cause)),
    ).catch((cause: unknown) => {
      console.error("Could not relaunch", api.describeError(cause));
      setRelaunching(false);
      setFailed(true);
    });
  }, [prepare]);

  return { relaunching, failed, relaunch };
}
