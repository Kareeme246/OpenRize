import { useCallback } from "react";
import { plural } from "../lib/format";
import type { UpdateTimeEntry } from "../lib/types";
import { useToast } from "./useToast";

/** "Category updated", "3 entries marked billable": what an entry edit did. */
export function describeUpdate(patch: UpdateTimeEntry, count = 1): string {
  const what = count === 1 ? "" : ` on ${plural(count, "entry", "entries")}`;
  if (patch.billable !== undefined) {
    const marked = patch.billable ? "billable" : "non-billable";
    return count === 1
      ? `Marked ${marked}`
      : `${plural(count, "entry", "entries")} marked ${marked}`;
  }
  if (patch.categoryId !== undefined) {
    return `Category ${patch.categoryId ? "set" : "cleared"}${what}`;
  }
  if (patch.projectId !== undefined) {
    return `Project ${patch.projectId ? "set" : "cleared"}${what}`;
  }
  if (patch.description !== undefined) return `Description saved${what}`;
  if (patch.startedAt !== undefined || patch.endedAt !== undefined) {
    return `Times updated${what}`;
  }
  return `Entry updated${what}`;
}

/**
 * Runs a change and, once it has gone through, confirms it with a toast.
 * Errors reach the caller untouched, for its inline error, and raise none.
 */
export function useChangeToast(): <T>(
  change: () => Promise<T>,
  message: string | ((value: T) => string),
) => Promise<T> {
  const showToast = useToast();
  return useCallback(
    async <T>(
      change: () => Promise<T>,
      message: string | ((value: T) => string),
    ): Promise<T> => {
      const value = await change();
      showToast({
        message: typeof message === "function" ? message(value) : message,
        tone: "success",
      });
      return value;
    },
    [showToast],
  );
}
