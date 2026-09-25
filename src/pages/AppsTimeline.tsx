import {
  type PointerEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Donut } from "../components/Charts";
import { DateStepper, EmptyState, InlineError } from "../components/Page";
import { Picker } from "../components/Picker";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { timeByApp } from "../lib/activity";
import * as api from "../lib/api";
import { addDays, startOfDay } from "../lib/dates";
import { formatDuration, formatTime } from "../lib/format";
import { assignColors } from "../lib/palette";
import type {
  ActivitySegment,
  AppRecord,
  Category,
  Project,
} from "../lib/types";
import { gutterLabel, timelineFor } from "./calendar/timeline";

interface Props {
  apps: AppRecord[];
  categories: Category[];
  projects: Project[];
  onUpdate: (
    app: AppRecord,
    changes: { defaultCategoryId?: string; defaultProjectId?: string },
  ) => void;
}

export function AppsTimeline({ apps, categories, projects, onUpdate }: Props) {
  const [day, setDay] = useState(() => startOfDay(new Date()));
  const [segments, setSegments] = useState<ActivitySegment[]>([]);
  const [now, setNow] = useState(Date.now);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [playhead, setPlayhead] = useState<number | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const dragging = useRef(false);
  const request = useRef(0);
  const dayStart = day.getTime();
  const dayEnd = addDays(day, 1).getTime();

  const refresh = useCallback(async () => {
    const id = ++request.current;
    try {
      const snapshot = await api.fetchActivitySnapshot(dayStart);
      if (id !== request.current) return;
      // The snapshot extends to now, even when viewing a previous day.
      setSegments(
        snapshot.segments.filter(
          (segment) =>
            segment.startedAt < dayEnd &&
            (segment.endedAt ?? Date.now()) > dayStart,
        ),
      );
      setNow(Date.now());
      setError(null);
    } catch (cause) {
      if (id === request.current) setError(api.describeError(cause));
    } finally {
      if (id === request.current) setLoading(false);
    }
  }, [dayStart, dayEnd]);

  useEffect(() => {
    setLoading(true);
    setSegments([]);
    setPlayhead(null);
    void refresh();
    return () => {
      request.current++;
    };
  }, [refresh]);
  useTauriEvent(api.ACTIVITY_CHANGED, () => void refresh());
  useTauriEvent(api.ACTIVITY_TICK, () => setNow(Date.now()));

  const visible = useMemo(
    () =>
      segments
        .filter(
          (s) =>
            s.kind !== "break" &&
            s.startedAt < dayEnd &&
            (s.endedAt ?? now) > dayStart,
        )
        .map((s) => ({
          ...s,
          startedAt: Math.max(dayStart, s.startedAt),
          endedAt: Math.min(dayEnd, s.endedAt ?? now),
        })),
    [segments, dayStart, dayEnd, now],
  );
  const totals = useMemo(() => timeByApp(visible, now), [visible, now]);
  const totalMs = totals.reduce((sum, row) => sum + row.ms, 0);
  const colors = useMemo(
    () => assignColors(totals.map((row) => row.app)),
    [totals],
  );
  const slices = totals.map(({ app, ms }) => ({
    key: app,
    label: app,
    ms,
    color: colors.get(app) ?? "var(--fg-faint)",
  }));
  const timeline = timelineFor(
    [
      ...visible.map((s) => ({
        start: s.startedAt,
        end: s.endedAt ?? now,
        dayStart,
      })),
      ...(now >= dayStart && now < dayEnd
        ? [{ start: now, end: now, dayStart }]
        : []),
    ],
    "elapsed",
    1,
    (dayEnd - dayStart) / 3_600_000,
  );
  const start = dayStart + timeline.startHour * 3_600_000;
  const end = dayStart + timeline.endHour * 3_600_000;
  const position = (time: number) =>
    `${((time - start) / (end - start)) * 100}%`;
  const defaultTime =
    now >= dayStart && now < dayEnd
      ? Math.min(Math.max(now, start), end)
      : Math.min(
          end,
          Math.max(start, (visible[visible.length - 1]?.endedAt ?? start) - 1),
        );
  const activeTime = hover ?? playhead ?? defaultTime;
  const activeApps = [
    ...new Set(
      visible
        .filter(
          (s) => s.startedAt <= activeTime && (s.endedAt ?? now) >= activeTime,
        )
        .map((s) => s.app),
    ),
  ];

  const scrub = (event: PointerEvent<HTMLDivElement>) => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const fraction = Math.min(
      1,
      Math.max(0, (event.clientX - bounds.left) / bounds.width),
    );
    const time = start + fraction * (end - start);
    setHover(time);
    if (dragging.current) setPlayhead(time);
  };

  return (
    <div className="space-y-4 pb-5">
      <section className="rounded-xl border border-line bg-surface p-4">
        <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold text-fg-strong">Timeline</h2>
            <p className="text-xs text-fg-muted">
              {day.toLocaleDateString(undefined, {
                weekday: "long",
                month: "long",
                day: "numeric",
                year: "numeric",
              })}
            </p>
          </div>
          <DateStepper
            unit="day"
            onStep={(direction) =>
              setDay((previous) => addDays(previous, direction))
            }
            onToday={() => setDay(startOfDay(new Date()))}
          />
        </div>
        {error && (
          <div className="mb-3">
            <InlineError message={error} onRetry={() => void refresh()} />
          </div>
        )}
        <div
          className="relative h-3 overflow-hidden rounded-sm bg-surface-strong"
          aria-hidden="true"
        >
          {visible.map((segment) => (
            <span
              key={segment.id}
              className="absolute inset-y-0"
              style={{
                left: position(segment.startedAt),
                width: `max(2px, ${((segment.endedAt - segment.startedAt) / (end - start)) * 100}%)`,
                backgroundColor: colors.get(segment.app),
              }}
            />
          ))}
        </div>
        <div
          role="slider"
          tabIndex={0}
          aria-label="Activity playhead"
          aria-valuemin={start}
          aria-valuemax={end}
          aria-valuenow={Math.round(playhead ?? defaultTime)}
          aria-valuetext={`${formatTime(activeTime)}: ${activeApps.join(", ") || "No activity"}`}
          className="relative mt-2 h-36 cursor-crosshair touch-none overflow-visible rounded-md border border-line bg-panel outline-none focus-visible:ring-2 focus-visible:ring-accent"
          onPointerDown={(event) => {
            dragging.current = true;
            event.currentTarget.setPointerCapture(event.pointerId);
            scrub(event);
          }}
          onPointerMove={scrub}
          onPointerUp={(event) => {
            scrub(event);
            dragging.current = false;
            event.currentTarget.releasePointerCapture(event.pointerId);
          }}
          onPointerCancel={() => {
            dragging.current = false;
            setHover(null);
          }}
          onPointerLeave={() => {
            if (!dragging.current) setHover(null);
          }}
          onKeyDown={(event) => {
            const step =
              event.key === "ArrowRight"
                ? 300_000
                : event.key === "ArrowLeft"
                  ? -300_000
                  : 0;
            if (step || event.key === "Home" || event.key === "End") {
              event.preventDefault();
              setPlayhead(
                event.key === "Home"
                  ? start
                  : event.key === "End"
                    ? end
                    : Math.max(
                        start,
                        Math.min(end, (playhead ?? defaultTime) + step),
                      ),
              );
            }
          }}
        >
          {timeline.hours.map((hour) => (
            <span
              key={hour}
              className="pointer-events-none absolute inset-y-0 border-l border-line-soft"
              style={{ left: position(dayStart + hour * 3_600_000) }}
            />
          ))}
          {visible.map((segment) => (
            <span
              key={segment.id}
              className="pointer-events-none absolute inset-y-0 opacity-85"
              style={{
                left: position(segment.startedAt),
                width: `max(2px, ${((segment.endedAt - segment.startedAt) / (end - start)) * 100}%)`,
                backgroundColor: colors.get(segment.app),
              }}
            />
          ))}
          {now >= start && now <= end && (
            <span
              className="pointer-events-none absolute inset-y-0 z-10 border-l border-dashed border-fg-soft"
              style={{ left: position(now) }}
            />
          )}
          <span
            className="pointer-events-none absolute inset-y-0 z-20 border-l-2 border-accent"
            style={{ left: position(playhead ?? defaultTime) }}
          >
            <span className="absolute -top-2 left-1/2 size-3 -translate-x-1/2 rounded-full border-2 border-panel bg-accent shadow-sm" />
          </span>
          <span
            className="pointer-events-none absolute -top-7 z-20 -translate-x-1/2 rounded bg-surface-strong px-2 py-1 text-[11px] font-medium tabular-nums text-fg-strong"
            style={{
              left: `clamp(32px, ${position(activeTime)}, calc(100% - 32px))`,
            }}
          >
            {formatTime(activeTime)}
          </span>
        </div>
        <div className="relative mt-2 h-4 text-[10px] text-fg-faint">
          {timeline.hours.map((hour) => (
            <span
              key={hour}
              className="absolute -translate-x-1/2 whitespace-nowrap"
              style={{
                left: `clamp(15px, ${position(dayStart + hour * 3_600_000)}, calc(100% - 15px))`,
              }}
            >
              {gutterLabel(hour, dayStart, "elapsed")}
            </span>
          ))}
        </div>
        <p className="mt-3 min-h-5 text-xs text-fg-muted" aria-live="polite">
          <span className="font-medium text-fg-strong">
            {formatTime(activeTime)}
          </span>{" "}
          · {activeApps.length ? activeApps.join(", ") : "No activity"}
        </p>
      </section>

      <div className="grid gap-4 lg:grid-cols-[240px_minmax(0,1fr)]">
        <section className="rounded-xl border border-line bg-surface p-4">
          <h2 className="mb-5 text-sm font-semibold text-fg-strong">
            Total tracked time
          </h2>
          <div className="flex justify-center">
            <Donut
              title="Apps & Websites"
              slices={slices}
              size={190}
              maxLegend={slices.length}
              showLegend={false}
            />
          </div>
        </section>
        <section className="min-w-0 rounded-xl border border-line bg-surface p-4">
          <h2 className="mb-3 text-sm font-semibold text-fg-strong">
            Apps & Websites
          </h2>
          {loading ? (
            <p className="py-8 text-center text-xs text-fg-muted">
              Loading activity...
            </p>
          ) : totals.length === 0 ? (
            <EmptyState title="No activity tracked this day" />
          ) : (
            <div className="max-w-full overflow-x-auto">
              <table className="w-full min-w-[480px] table-fixed text-left text-xs">
                <thead>
                  <tr className="border-b border-line text-[11px] text-fg-faint">
                    <th className="w-24 pb-2 font-medium">Share</th>
                    <th className="pb-2 font-medium">
                      Application / Website · Defaults
                    </th>
                    <th className="w-20 pb-2 text-right font-medium">
                      Duration
                    </th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-line-soft">
                  {totals.map(({ app, ms }) => {
                    const sample = visible.find(
                      (segment) => segment.app === app,
                    );
                    const record =
                      apps.find(
                        (item) =>
                          item.id === sample?.appId ||
                          item.identifier === sample?.domain ||
                          item.identifier === sample?.bundleId,
                      ) ??
                      apps.find(
                        (item) =>
                          item.displayName === app || item.identifier === app,
                      );
                    return (
                      <tr key={app} className="hover:bg-surface-strong/40">
                        <td className="py-2 pr-3 tabular-nums text-fg-soft">
                          <span className="inline-block w-8">
                            {totalMs
                              ? `${Math.round((ms / totalMs) * 100)}%`
                              : "0%"}
                          </span>
                          <span className="inline-block h-1.5 w-12 overflow-hidden rounded-full bg-surface-strong align-middle">
                            <span
                              className="block h-full rounded-full"
                              style={{
                                width: `${(ms / totalMs) * 100}%`,
                                backgroundColor: colors.get(app),
                              }}
                            />
                          </span>
                        </td>
                        <td className="py-2 pr-3 text-fg-strong">
                          <div className="mb-1.5 flex items-center gap-2 font-medium">
                            <span
                              className="inline-block size-2 shrink-0 rounded-full"
                              style={{ backgroundColor: colors.get(app) }}
                            />
                            <span className="truncate">{app}</span>
                          </div>
                          {record ? (
                            <div className="flex flex-wrap gap-2">
                              <Picker
                                ariaLabel={`Default category for ${app}`}
                                value={record.defaultCategoryId || ""}
                                disabled={record.excluded}
                                onChange={(value) =>
                                  onUpdate(record, {
                                    defaultCategoryId: value || undefined,
                                  })
                                }
                                options={[
                                  { value: "", label: "(None)" },
                                  ...categories.map((c) => ({
                                    value: c.id,
                                    label: c.name,
                                    color: c.color,
                                  })),
                                ]}
                                variant="compact"
                              />
                              <Picker
                                ariaLabel={`Default project for ${app}`}
                                value={record.defaultProjectId || ""}
                                disabled={record.excluded}
                                onChange={(value) =>
                                  onUpdate(record, {
                                    defaultProjectId: value || undefined,
                                  })
                                }
                                options={[
                                  { value: "", label: "(None)" },
                                  ...projects.map((p) => ({
                                    value: p.id,
                                    label: p.name,
                                    color: p.color,
                                  })),
                                ]}
                                variant="compact"
                              />
                            </div>
                          ) : (
                            <span className="text-fg-faint">
                              Defaults unavailable
                            </span>
                          )}
                        </td>
                        <td className="py-2 text-right font-mono tabular-nums text-fg-soft">
                          {formatDuration(ms)}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
