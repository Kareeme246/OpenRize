import { hasPrimaryModifier } from "./platform.ts";

/**
 * The entry review panel's window-level keys. Merging has none: it joins a
 * neighbour on one side, which a bare key cannot say.
 */

export type EntryShortcut = "accept" | "reject" | "split" | "close";

export interface ShortcutKey {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}

export function entryShortcut(event: ShortcutKey): EntryShortcut | undefined {
  const command = hasPrimaryModifier(event);
  if (event.key === "Escape") return "close";
  if (command && event.key === "Enter") return "accept";
  if (command && event.key === "Backspace") return "reject";
  if (
    !event.metaKey &&
    !event.ctrlKey &&
    !event.altKey &&
    event.key.toLowerCase() === "s"
  ) {
    return "split";
  }
  return undefined;
}
