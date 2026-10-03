/** Pack each connected overlap group independently; isolated entries use full width. */
export function packOverlaps<T extends { startedAt: number; endedAt: number }>(
  items: readonly T[],
): { item: T; column: number; columns: number }[] {
  const sorted = items
    .map((item, index) => ({ item, index }))
    .sort((a, b) => a.item.startedAt - b.item.startedAt || a.index - b.index);
  const result: { item: T; column: number; columns: number }[] = [];
  let group: { item: T; column: number }[] = [];
  let ends: number[] = [];
  let groupEnd = -Infinity;
  const flush = (): void => {
    for (const entry of group) result.push({ ...entry, columns: ends.length });
    group = [];
    ends = [];
  };
  for (const { item } of sorted) {
    if (item.startedAt >= groupEnd) flush();
    let column = ends.findIndex((end) => end <= item.startedAt);
    if (column === -1) column = ends.length;
    ends[column] = item.endedAt;
    groupEnd = Math.max(groupEnd, item.endedAt);
    group.push({ item, column });
  }
  flush();
  return result;
}
