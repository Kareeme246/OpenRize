/**
 * The entry review panel's window-level keys. Merging deliberately has none:
 * it joins an entry to a neighbour on one side, which a bare key cannot say.
 * The panel itself owns 1-3, C, P and E.
 */

export type EntryShortcut = "accept" | "reject" | "split" | "close";

/** The parts of a `KeyboardEvent` a shortcut reads. */
export interface ShortcutKey {
  key: string;
  metaKey: boolean;
  ctrlKey: boolean;
  altKey: boolean;
}

/** What a key press asks the open entry to do, if anything. */
export function entryShortcut(event: ShortcutKey): EntryShortcut | undefined {
  const command = event.metaKey || event.ctrlKey;
  if (event.key === "Escape") return "close";
  if (command && event.key === "Enter") return "accept";
  if (command && event.key === "Backspace") return "reject";
  if (!command && !event.altKey && event.key.toLowerCase() === "s") {
    return "split";
  }
  return undefined;
}
