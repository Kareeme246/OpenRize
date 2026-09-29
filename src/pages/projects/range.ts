import {
  addDays,
  startOfDay,
  startOfMonth,
  startOfWeek,
} from "../../lib/dates";
import type { ProjectsRange } from "../../lib/types";

/** The time-range choices Projects and Clients offer for their Time column. */
export const RANGE_OPTIONS: { value: ProjectsRange; label: string }[] = [
  { value: "week", label: "This week" },
  { value: "month", label: "This month" },
  { value: "30d", label: "Last 30 days" },
  { value: "all", label: "All time" },
];

export function rangeBounds(range: ProjectsRange): {
  start: number;
  end: number;
} {
  const now = new Date();
  const end = addDays(startOfDay(now), 1).getTime();
  if (range === "week") return { start: startOfWeek(now).getTime(), end };
  if (range === "month") return { start: startOfMonth(now).getTime(), end };
  if (range === "30d")
    return { start: addDays(startOfDay(now), -29).getTime(), end };
  return { start: 0, end };
}
