import type { TimeEntry } from "./types";

export interface ProjectWeek {
  projectId: string;
  days: number[];
  approvedMs: number;
  billableMs: number;
}

/** Local-midnight edges (including DST changes) partition overlapping entries. */
export function projectWeek(
  entries: TimeEntry[],
  edges: number[],
): ProjectWeek[] {
  const rows = new Map<string, ProjectWeek>();
  for (const entry of entries) {
    if (
      !entry.projectId ||
      entry.endedAt <= entry.startedAt ||
      entry.endedAt <= edges[0] ||
      entry.startedAt >= edges[edges.length - 1]
    )
      continue;
    let row = rows.get(entry.projectId);
    if (!row) {
      row = {
        projectId: entry.projectId,
        days: Array(edges.length - 1).fill(0),
        approvedMs: 0,
        billableMs: 0,
      };
      rows.set(entry.projectId, row);
    }
    for (let day = 0; day < edges.length - 1; day++) {
      const duration = Math.max(
        0,
        Math.min(entry.endedAt, edges[day + 1]) -
          Math.max(entry.startedAt, edges[day]),
      );
      row.days[day] += duration;
      if (entry.status === "approved") {
        row.approvedMs += duration;
        if (entry.billable) row.billableMs += duration;
      }
    }
  }
  return [...rows.values()].sort((a, b) =>
    a.projectId.localeCompare(b.projectId),
  );
}
