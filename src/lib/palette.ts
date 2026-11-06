/**
 * The muted categorical palette. Entities without a colour of their own (apps,
 * clients) hash into it by name, so colour follows the entity, not its chart
 * rank.
 */
export const PALETTE = [
  "#75a4e5",
  "#56c2b1",
  "#52b788",
  "#df84b5",
  "#e4817d",
  "#9aa6b4",
  "#bfa181",
  "#707bf0",
  "#66b1df",
  "#9b87df",
];

function slotFor(key: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) {
    hash ^= key.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0) % PALETTE.length;
}

/**
 * Colours for one chart's entities: each key starts at its hashed slot and
 * steps to the next free one on a clash; keys claim slots in name order.
 */
export function assignColors(keys: Iterable<string>): Map<string, string> {
  const colors = new Map<string, string>();
  const taken = new Set<number>();
  for (const key of [...new Set(keys)].sort()) {
    let slot = slotFor(key);
    for (let step = 0; step < PALETTE.length && taken.has(slot); step++) {
      slot = (slot + 1) % PALETTE.length;
    }
    taken.add(slot);
    colors.set(key, PALETTE[slot]);
  }
  return colors;
}
