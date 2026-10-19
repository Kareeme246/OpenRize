import { useCallback } from "react";
import * as api from "../lib/api";
import { plural } from "../lib/format";
import type { UpdateTimeEntry } from "../lib/types";
import { useToast } from "./useToast";

export interface ChangeToast {
  message: string;
  /** False where the store refuses the inverse, e.g. removing approved time. */
  undoable?: boolean;
}

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
 * Runs a change to entries or the catalog and confirms it with a toast whose
 * Undo takes back exactly that change, through the store's own undo history
 * (the same one ⌘Z walks). The history's head is read before and after: a
 * change that recorded nothing gets no Undo, and `undo_change` refuses once
 * anything else has been changed since, so the button can never take back
 * the wrong thing. Errors reach the caller untouched, for its inline error.
 */
export function useChangeToast(): <T>(
  change: () => Promise<T>,
  confirm: ChangeToast | ((value: T) => ChangeToast),
) => Promise<T> {
  const showToast = useToast();
  return useCallback(
    async <T>(
      change: () => Promise<T>,
      confirm: ChangeToast | ((value: T) => ChangeToast),
    ): Promise<T> => {
      const before = await api.undoHead().catch(() => null);
      const value = await change();
      const { message, undoable = true } =
        typeof confirm === "function" ? confirm(value) : confirm;
      const after = await api.undoHead().catch(() => null);
      const seq =
        undoable && after !== null && after.seq !== before?.seq
          ? after.seq
          : null;
      showToast({
        message,
        tone: "success",
        action:
          seq === null
            ? undefined
            : {
                label: "Undo",
                onAction: () => {
                  api
                    .undoChange(seq)
                    .then((label) =>
                      showToast({ message: `Undid ${label.toLowerCase()}` }),
                    )
                    .catch((cause: unknown) =>
                      showToast({
                        message: `Couldn't undo: ${api.describeError(cause)}`,
                        tone: "error",
                      }),
                    );
                },
              },
      });
      return value;
    },
    [showToast],
  );
}

/** How to take a confirmed change back outside the entry history. */
export interface Inverse {
  run: () => Promise<unknown>;
  /** What the toast says once it has been taken back. */
  undone: string;
}

/**
 * Confirms a change the app takes back itself: writing back the previous
 * value or putting back the rows it removed. Without `inverse` the toast
 * only confirms. A failed undo says so; nothing is retried behind the scenes.
 */
export function useInverseToast(): (
  message: string,
  inverse?: Inverse,
) => void {
  const showToast = useToast();
  return useCallback(
    (message: string, inverse?: Inverse): void => {
      showToast({
        message,
        tone: "success",
        action: inverse && {
          label: "Undo",
          onAction: () => {
            inverse
              .run()
              .then(() => showToast({ message: inverse.undone }))
              .catch((cause: unknown) =>
                showToast({
                  message: `Couldn't undo: ${api.describeError(cause)}`,
                  tone: "error",
                }),
              );
          },
        },
      });
    },
    [showToast],
  );
}
