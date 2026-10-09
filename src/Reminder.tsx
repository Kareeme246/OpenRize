import {
  type ReactNode,
  type RefObject,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useBreaks } from "./hooks/useBreaks";
import { SettingsProvider, useSettings } from "./hooks/useSettings";
import * as api from "./lib/api";
import {
  type BreakState,
  type BreakView,
  formatCountdown,
  formatPlanned,
  formatWorked,
  type ReminderView,
  SNOOZE_CHOICES,
  type StopwatchReminderView,
} from "./lib/breaks";

const HOUR_MS = 3_600_000;

/**
 * The positioned panel: break reminders, the elapsed-time capsule, the
 * welcome-back card, and stopwatch reminders. Rust decides when it is on
 * screen; this draws the state and reports its size so the window fits it.
 */
export default function Reminder() {
  return (
    <SettingsProvider>
      <ReminderPanel />
    </SettingsProvider>
  );
}

function ReminderPanel() {
  const { state, now } = useBreaks();
  const { settings } = useSettings();
  // A panel in a bottom corner opens its menus upwards, and is pinned to the
  // window's bottom edge, which Rust keeps fixed as the window grows to fit.
  const upward =
    settings.notificationPlacement === "bottomLeft" ||
    settings.notificationPlacement === "bottomRight";
  const [menu, setMenu] = useState<"snooze" | "more" | null>(null);
  const [collapsed, setCollapsed] = useState<{
    id: string | null;
    value: boolean;
  }>({ id: null, value: false });
  const currentId = state.current?.id ?? null;
  const compact =
    state.current?.endedAt === null &&
    collapsed.id === currentId &&
    collapsed.value;

  // A menu belongs to the reminder that opened it.
  useEffect(() => {
    if (state.phase !== "due") setMenu(null);
  }, [state.phase]);

  const card = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const reported = useRef("");
  // Report the card and any overhanging menu's bounds after every state
  // change: the window waits for a measurement before it shows.
  // biome-ignore lint/correctness/useExhaustiveDependencies: state, menu and compact are what change the layout
  useLayoutEffect(() => {
    const element = card.current;
    // No break or stopwatch card means there is nothing to size a window to.
    if (!element || (state.phase === "idle" && !state.stopwatch)) return;
    const measure = (): {
      height: number;
      signature: string;
      width: number;
    } => {
      const box = element.getBoundingClientRect();
      const menuBox = menuRef.current?.getBoundingClientRect();
      const left = Math.min(box.left, menuBox?.left ?? box.left);
      const right = Math.max(box.right, menuBox?.right ?? box.right);
      const top = Math.min(box.top, menuBox?.top ?? box.top);
      const bottom = Math.max(box.bottom, menuBox?.bottom ?? box.bottom);
      const width = Math.ceil(right - left);
      const height = Math.ceil(bottom - top);
      return { height, signature: `${left},${top},${width}x${height}`, width };
    };
    const report = (): void => {
      const { height, signature, width } = measure();
      reported.current = signature;
      void api.resizeReminderPanel(width, height);
    };
    report();
    const observer = new ResizeObserver(() => {
      if (measure().signature !== reported.current) report();
    });
    observer.observe(element);
    if (menuRef.current) observer.observe(menuRef.current);
    return () => observer.disconnect();
  }, [state, menu, compact]);

  return (
    <div className={`fixed right-0 ${upward ? "bottom-0" : "top-0"}`}>
      <div
        ref={card}
        className={`w-max select-none border border-line-strong bg-panel text-[12px] text-fg ${compact ? "rounded-full" : "rounded-[14px]"}`}
      >
        <PanelBody
          state={state}
          now={now}
          menu={menu}
          menuRef={menuRef}
          onMenu={setMenu}
          upward={upward}
          expanded={!compact}
          onToggle={() => setCollapsed({ id: currentId, value: !compact })}
        />
      </div>
    </div>
  );
}

function PanelBody({
  state,
  now,
  menu,
  menuRef,
  onMenu,
  upward,
  expanded,
  onToggle,
}: {
  state: BreakState;
  now: number;
  menu: "snooze" | "more" | null;
  menuRef: RefObject<HTMLDivElement | null>;
  onMenu: (menu: "snooze" | "more" | null) => void;
  upward: boolean;
  expanded: boolean;
  onToggle: () => void;
}) {
  switch (state.phase) {
    case "idle":
      return state.stopwatch ? (
        <StopwatchCard reminder={state.stopwatch} now={now} />
      ) : null;
    case "due":
      return state.reminder ? (
        <ReminderCard
          reminder={state.reminder}
          menu={menu}
          menuRef={menuRef}
          onMenu={onMenu}
          upward={upward}
        />
      ) : null;
    case "nudge":
      return state.reminder ? <NudgeCapsule reminder={state.reminder} /> : null;
    case "onBreak":
    case "over":
      return state.current ? (
        state.current.endedAt !== null ? (
          <WelcomeCard current={state.current} />
        ) : (
          <BreakCard
            current={state.current}
            now={now}
            expanded={expanded}
            onToggle={onToggle}
          />
        )
      ) : null;
    default:
      return null;
  }
}

function StopwatchCard({
  reminder,
  now,
}: {
  reminder: StopwatchReminderView;
  now: number;
}) {
  return (
    <div className="w-[300px] px-4 py-3">
      <div className="flex items-start gap-2.5">
        <span className="mt-[5px] size-2 shrink-0 rounded-full bg-review" />
        <div className="min-w-0 flex-1 break-words font-semibold text-[13.5px] text-fg-strong leading-snug">
          {reminder.label} has run {formatWorked(now - reminder.startedAt)}
        </div>
      </div>
      <div className="mt-3 flex items-center gap-1.5">
        <button
          type="button"
          onClick={() => act(api.pauseTimer(reminder.id))}
          className="rounded-lg bg-accent px-3 py-1.5 font-semibold text-[12px] text-accent-fg transition-opacity hover:opacity-90"
        >
          Pause
        </button>
        <button
          type="button"
          onClick={() =>
            act(api.dismissStopwatchReminder(reminder.id, reminder.startedAt))
          }
          className="rounded-lg px-2.5 py-1.5 font-medium text-[12px] text-fg-soft transition-colors hover:bg-surface-strong hover:text-fg"
        >
          Keep going
        </button>
      </div>
    </div>
  );
}

/** Fire and forget: Rust answers with a `break-state-changed` event. */
function act(action: Promise<unknown>): void {
  action.catch((cause: unknown) => console.error("break action failed", cause));
}

function ReminderCard({
  reminder,
  menu,
  menuRef,
  onMenu,
  upward,
}: {
  reminder: ReminderView;
  menu: "snooze" | "more" | null;
  menuRef: RefObject<HTMLDivElement | null>;
  onMenu: (menu: "snooze" | "more" | null) => void;
  upward: boolean;
}) {
  const scheduled = reminder.source === "scheduled";
  const title = scheduled
    ? `${reminder.label} · ${formatPlanned(reminder.plannedMs).replace("-", " ")}`
    : `Time for a ${formatPlanned(reminder.plannedMs)} break`;
  const startLabel = scheduled
    ? `Start ${reminder.label.toLowerCase()}`
    : "Start";
  return (
    <div className="w-[360px] px-4 py-3.5">
      <div className="flex items-start gap-2.5">
        <span className="mt-[5px] size-2 shrink-0 rounded-full bg-review" />
        <div className="min-w-0 flex-1">
          <div className="font-semibold text-[13.5px] text-fg-strong leading-snug">
            {title}
          </div>
          {!scheduled && (
            <div className="mt-0.5 text-[12px] text-fg-muted">
              You've worked {formatWorked(reminder.workedMs)} straight.
            </div>
          )}
          {reminder.message && (
            <div className="mt-1 text-[12px] text-fg-soft">
              {reminder.message}
            </div>
          )}
        </div>
      </div>
      <div className="mt-3 flex items-center gap-1.5">
        <button
          type="button"
          onClick={() => act(api.startBreak())}
          className="shrink-0 whitespace-nowrap rounded-lg bg-accent px-3 py-1.5 font-semibold text-[12px] text-accent-fg transition-opacity hover:opacity-90"
        >
          {startLabel}
        </button>
        {reminder.canSnooze &&
          (scheduled ? (
            <button
              type="button"
              onClick={() => act(api.snoozeBreak(15))}
              className="shrink-0 whitespace-nowrap rounded-lg border border-line bg-surface px-3 py-1.5 font-medium text-[12px] text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
            >
              In 15 min
            </button>
          ) : (
            <div className="relative">
              <SnoozeButton
                minutes={reminder.snoozeMinutes}
                open={menu === "snooze"}
                onToggle={() => onMenu(menu === "snooze" ? null : "snooze")}
              />
              {menu === "snooze" && (
                <Menu align="left" upward={upward} menuRef={menuRef}>
                  {SNOOZE_CHOICES.map((minutes) => (
                    <MenuItem
                      key={minutes}
                      onClick={() => act(api.snoozeBreak(minutes))}
                    >
                      Snooze {minutes} min
                    </MenuItem>
                  ))}
                </Menu>
              )}
            </div>
          ))}
        <button
          type="button"
          onClick={() => act(api.skipBreak())}
          className="shrink-0 whitespace-nowrap rounded-lg px-2.5 py-1.5 font-medium text-[12px] text-fg-soft transition-colors hover:bg-surface-strong hover:text-fg"
        >
          {scheduled ? "Skip today" : "Skip"}
        </button>
        <div className="relative ml-auto">
          <button
            type="button"
            aria-label="More options"
            aria-expanded={menu === "more"}
            onClick={() => onMenu(menu === "more" ? null : "more")}
            className="grid size-7 place-items-center rounded-lg text-fg-soft transition-colors hover:bg-surface-strong hover:text-fg"
          >
            <svg
              viewBox="0 0 24 24"
              className="size-4 fill-current"
              aria-hidden
            >
              <circle cx="5" cy="12" r="1.7" />
              <circle cx="12" cy="12" r="1.7" />
              <circle cx="19" cy="12" r="1.7" />
            </svg>
          </button>
          {menu === "more" && (
            <Menu align="right" upward={upward} menuRef={menuRef}>
              <MenuItem
                onClick={() =>
                  act(api.pauseBreakReminders(Date.now() + HOUR_MS))
                }
              >
                Pause reminders for 1 hour
              </MenuItem>
              <MenuItem
                onClick={() => act(api.pauseBreakReminders(tomorrow()))}
              >
                Pause reminders until tomorrow
              </MenuItem>
              <MenuItem onClick={() => act(api.openBreakSettings())}>
                Reminder settings…
              </MenuItem>
            </Menu>
          )}
        </div>
      </div>
    </div>
  );
}

/** Local midnight at the start of tomorrow. */
function tomorrow(): number {
  const date = new Date();
  date.setHours(0, 0, 0, 0);
  date.setDate(date.getDate() + 1);
  return date.getTime();
}

/**
 * A combo button: the main half snoozes for the default length, the chevron
 * offers 5 / 10 / 15 minutes on every reminder.
 */
function SnoozeButton({
  minutes,
  open,
  onToggle,
}: {
  minutes: number;
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <div className="flex rounded-lg border border-line bg-surface">
      <button
        type="button"
        onClick={() => act(api.snoozeBreak(minutes))}
        className="shrink-0 whitespace-nowrap rounded-l-[calc(var(--radius-lg)-1px)] px-3 py-1.5 font-medium text-[12px] text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
      >
        Snooze {minutes} min
      </button>
      <button
        type="button"
        aria-label="Choose snooze length"
        aria-expanded={open}
        onClick={onToggle}
        className={`grid w-7 place-items-center rounded-r-[calc(var(--radius-lg)-1px)] border-line border-l transition-colors hover:bg-surface-strong hover:text-fg ${
          open ? "bg-surface-strong text-fg" : "text-fg-soft"
        }`}
      >
        <svg
          viewBox="0 0 24 24"
          className="size-3.5"
          fill="none"
          stroke="currentColor"
          strokeWidth={2.4}
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden
        >
          <path d="M6 9l6 6 6-6" />
        </svg>
      </button>
    </div>
  );
}

function Menu({
  align,
  upward,
  menuRef,
  children,
}: {
  align: "left" | "right";
  upward: boolean;
  menuRef: RefObject<HTMLDivElement | null>;
  children: ReactNode;
}) {
  return (
    <div
      ref={menuRef}
      className={`absolute z-50 flex w-fit min-w-[150px] flex-col rounded-lg border border-line bg-panel p-1 ${
        upward ? "bottom-full mb-1" : "top-full mt-1"
      } ${align === "right" ? "right-0" : "left-0"}`}
    >
      {children}
    </div>
  );
}

function MenuItem({
  onClick,
  children,
}: {
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="whitespace-nowrap rounded-md px-2.5 py-1.5 text-left text-[12px] text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
    >
      {children}
    </button>
  );
}

/** The reminder went unanswered: a small "Break due" tab in the corner. */
function NudgeCapsule({ reminder }: { reminder: ReminderView }) {
  return (
    <button
      type="button"
      onClick={() => act(api.expandBreakReminder())}
      title="Show the break reminder"
      className="flex h-[34px] w-[190px] items-center gap-2 px-3.5 text-left transition-colors hover:bg-surface"
    >
      <span className="size-2 shrink-0 animate-pulse rounded-full bg-review" />
      <span className="min-w-0 flex-1 truncate font-semibold text-[12px] text-fg-strong">
        {reminder.source === "scheduled" ? reminder.label : "Break"} due
      </span>
      <span className="text-[11px] text-fg-faint">Open</span>
    </button>
  );
}

/** Click the live line to collapse or expand without changing the break. */
function BreakCard({
  current,
  now,
  expanded,
  onToggle,
}: {
  current: BreakView;
  now: number;
  expanded: boolean;
  onToggle: () => void;
}) {
  const end = current.startedAt + current.plannedMs;
  const over = now >= end;
  const progress = Math.min(
    1,
    Math.max(0, (now - current.startedAt) / current.plannedMs),
  );
  return (
    <div className={expanded ? "w-[280px]" : "max-w-[220px]"}>
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={expanded}
        aria-label={expanded ? "Collapse break tile" : "Expand break tile"}
        className="flex min-h-[38px] w-full items-center gap-2 rounded-full px-4 py-2 text-left hover:bg-surface"
      >
        <span
          className={`size-2 shrink-0 rounded-full bg-break ${over ? "" : "animate-pulse"}`}
        />
        <span className="min-w-0 flex-1 truncate font-semibold text-[12px] text-fg-strong">
          {over ? `${current.label} over` : current.label}
        </span>
        <span className="font-semibold text-[12px] text-break tabular-nums">
          {formatCountdown(now - current.startedAt)}
        </span>
      </button>
      {expanded && (
        <div className="mx-4 h-[2px] overflow-hidden rounded-full bg-line">
          <div
            className="h-full bg-break transition-[width] duration-1000 ease-linear"
            style={{ width: `${progress * 100}%` }}
          />
        </div>
      )}
      {expanded && (
        <div className="px-4 py-2.5">
          {over && (
            <div className="mb-2 text-[11.5px] text-fg-muted">
              Welcome back when you're ready.
            </div>
          )}
          {current.pausedTimers.length > 0 && (
            <div className="mb-2 text-[11px] text-fg-faint">
              Paused: {current.pausedTimers.join(", ")}
            </div>
          )}
          <div className="flex gap-1.5">
            <button
              type="button"
              onClick={() => act(api.endBreak())}
              className="rounded-lg bg-accent px-3 py-1.5 font-semibold text-[12px] text-accent-fg transition-opacity hover:opacity-90"
            >
              End break
            </button>
            <button
              type="button"
              onClick={() => act(api.extendBreak())}
              className="rounded-lg border border-line bg-surface px-3 py-1.5 font-medium text-[12px] text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
            >
              +5 min
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

function WelcomeCard({ current }: { current: BreakView }) {
  const took = Math.max(
    1,
    Math.round(
      ((current.endedAt ?? current.startedAt) - current.startedAt) / 60_000,
    ),
  );
  return (
    <div className="w-[300px] px-4 py-3">
      <div className="flex items-center gap-2.5">
        <span className="size-2 shrink-0 rounded-full bg-success" />
        <div className="font-semibold text-[13.5px] text-fg-strong">
          Welcome back · {took}m break
        </div>
      </div>
      {current.note && (
        <div className="mt-1 text-[11.5px] text-fg-muted">{current.note}</div>
      )}
      {current.resumedTimers.length > 0 && (
        <div className="mt-1 text-[11.5px] text-fg-muted">
          Resumed {current.resumedTimers.join(", ")}
        </div>
      )}
    </div>
  );
}
