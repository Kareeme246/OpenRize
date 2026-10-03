export interface CalendarViewport {
  top: number;
  left: number;
  hourHeight?: number;
}

// Session-only viewer state: no activity or user preferences are stored here.
const viewports = new Map<string, CalendarViewport>();
export function readViewport(key: string): CalendarViewport | undefined {
  return viewports.get(key);
}
export function rememberViewport(
  key: string,
  viewport: CalendarViewport,
): void {
  viewports.set(key, viewport);
  if (viewports.size > 100)
    viewports.delete(viewports.keys().next().value ?? "");
}
