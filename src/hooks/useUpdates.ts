import { useEffect, useState } from "react";
import * as api from "../lib/api";
import type { UpdateStatus } from "../lib/types";
import { useTauriEvent } from "./useTauriEvent";

/** The updater's status, kept live by `update-status`. */
export function useUpdates(): UpdateStatus | null {
  const [status, setStatus] = useState<UpdateStatus | null>(null);

  useEffect(() => {
    let active = true;
    api
      .updateStatus()
      .then((loaded) => {
        if (active) setStatus(loaded);
      })
      .catch((err: unknown) =>
        console.error("Failed to load update status", api.describeError(err)),
      );
    return () => {
      active = false;
    };
  }, []);
  useTauriEvent<UpdateStatus>(api.UPDATE_STATUS, setStatus);

  return status;
}
