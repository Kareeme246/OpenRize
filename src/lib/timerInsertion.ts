export interface TimerInsertion {
  slot: number;
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Slots refer to the unchanged reading order, including the lifted card.
 * Removing that card adjusts the insertion index only when the drop commits. */
export function timerInsertion(
  cards: DOMRect[],
  bounds: DOMRect,
  x: number,
  y: number,
  columnGap: number,
  rowGap: number,
): TimerInsertion | null {
  if (cards.length === 0) return null;
  const rows: { start: number; end: number; top: number; bottom: number }[] =
    [];
  for (const [index, card] of cards.entries()) {
    const row = rows[rows.length - 1];
    if (row && Math.abs(row.top - card.top) < 1) {
      row.end = index;
      row.bottom = Math.max(row.bottom, card.bottom);
    } else {
      rows.push({
        start: index,
        end: index,
        top: card.top,
        bottom: card.bottom,
      });
    }
  }
  const singleColumn = rows.every((row) => row.start === row.end);
  let slot = cards.length;
  for (const row of rows) {
    // The whole space between rows maps to their shared insertion slot.
    if (y < row.top) {
      slot = row.start;
      break;
    }
    if (y > row.bottom) continue;
    slot = row.end + 1;
    for (let index = row.start; index <= row.end; index++) {
      const card = cards[index];
      if (
        singleColumn
          ? y < card.top + card.height / 2
          : x < card.left + card.width / 2
      ) {
        slot = index;
        break;
      }
    }
    break;
  }

  const before = cards[slot - 1];
  const after = cards[slot];
  const edge = after ?? before;
  const rowBoundary = before && after && Math.abs(before.top - after.top) >= 1;
  if (singleColumn || rowBoundary) {
    const center = before
      ? after
        ? (before.bottom + after.top) / 2
        : before.bottom + rowGap / 2
      : after.top - rowGap / 2;
    return {
      slot,
      left: singleColumn ? edge.left - bounds.left : 0,
      top: center - bounds.top - 1,
      width: singleColumn ? edge.width : bounds.width,
      height: 2,
    };
  }

  // Within a row (including the outer ends), the gap runs vertically.
  const center = before
    ? after
      ? (before.right + after.left) / 2
      : before.right + columnGap / 2
    : after.left - columnGap / 2;
  return {
    slot,
    left: center - bounds.left - 1,
    top: edge.top - bounds.top,
    width: 2,
    height: edge.height,
  };
}
