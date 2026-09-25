/**
 * The muted categorical palette (design board, theme T). Projects pick from
 * it; entities without a colour of their own (apps, clients) get one from it
 * by name, so a colour follows its entity rather than its rank in a chart.
 */
export const PALETTE = [
  "#75a4e5",
  "#56c2b1",
  "#e5995c",
  "#df84b5",
  "#e4817d",
  "#9aa6b4",
  "#bfa181",
  "#e7b447",
  "#66b1df",
  "#9b87df",
];

/** The palette slot `key` hashes to (FNV-1a over its UTF-16 code units). */
function slotFor(key: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) {
    hash ^= key.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0) % PALETTE.length;
}

/**
 * Colours for the entities one chart shows together. Each key starts at the
 * slot its name hashes to, so it keeps its colour as filters change, and
 * steps to the next free slot on a clash so no two share one (until there
 * are more keys than colours). Keys claim slots in name order, not rank.
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
