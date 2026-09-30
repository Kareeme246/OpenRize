import {
  type ReactNode,
  type RefObject,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useBreaks } from "./hooks/useBreaks";
import { SettingsProvider } from "./hooks/useSettings";
import * as api from "./lib/api";
import {
  type BreakState,
  type BreakView,
  formatCountdown,
  formatPlanned,
  formatWorked,
  type ReminderView,
  SNOOZE_CHOICES,
} from "./lib/breaks";

const HOUR_MS = 3_600_000;

/**
 * The top-right break panel: the reminder, the countdown capsule, and the
 * welcome-back card. Rust decides when it is on screen; this only draws the
 * state and reports its own size so the window can be fitted to it.
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
  const [menu, setMenu] = useState<"snooze" | "more" | null>(null);
  const [hovered, setHovered] = useState(false);
  const leave = useRef<number | undefined>(undefined);

  // A menu belongs to the reminder that opened it.
  useEffect(() => {
    if (state.phase !== "due") setMenu(null);
  }, [state.phase]);

  const card = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const reported = useRef("");
  // Report the card and any overhanging menu's bounds after every state
  // change: the window waits for a measurement before it shows.
  // biome-ignore lint/correctness/useExhaustiveDependencies: state, menu and hover are what change the layout
  useLayoutEffect(() => {
    const element = card.current;
    // Nothing is drawn while idle, so there is nothing to size a window to.
    if (!element || state.phase === "idle") return;
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
  }, [state, menu, hovered]);

  // Hover opens the break controls. Listening on the element keeps the
  // card a plain container for the linter and screen readers.
  useEffect(() => {
    const element = card.current;
    if (!element) return;
    const enter = (): void => {
      window.clearTimeout(leave.current);
      setHovered(true);
    };
    const exit = (): void => {
      leave.current = window.setTimeout(() => setHovered(false), 250);
    };
    element.addEventListener("mouseenter", enter);
    element.addEventListener("mouseleave", exit);
    return () => {
      window.clearTimeout(leave.current);
      element.removeEventListener("mouseenter", enter);
      element.removeEventListener("mouseleave", exit);
    };
  }, []);

  return (
    <div className="fixed top-0 right-0">
      <div
        ref={card}
        className="w-max select-none rounded-[14px] border border-line-strong bg-panel text-[12px] text-fg"
      >
        <PanelBody
          state={state}
          now={now}
          menu={menu}
          menuRef={menuRef}
          onMenu={setMenu}
          expanded={hovered}
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
  expanded,
}: {
  state: BreakState;
  now: number;
  menu: "snooze" | "more" | null;
  menuRef: RefObject<HTMLDivElement | null>;
  onMenu: (menu: "snooze" | "more" | null) => void;
  expanded: boolean;
}) {
  switch (state.phase) {
    case "due":
      return state.reminder ? (
        <ReminderCard
          reminder={state.reminder}
          menu={menu}
          menuRef={menuRef}
          onMenu={onMenu}
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
          <BreakCard current={state.current} now={now} expanded={expanded} />
        )
      ) : null;
    default:
      return null;
  }
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
}: {
  reminder: ReminderView;
  menu: "snooze" | "more" | null;
  menuRef: RefObject<HTMLDivElement | null>;
  onMenu: (menu: "snooze" | "more" | null) => void;
}) {
  const scheduled = reminder.source === "scheduled";
  const title = scheduled
    ? `${reminder.label} · ${formatPlanned(reminder.plannedMs).replace("-", " ")}`
    : `Time for a ${formatPlanned(reminder.plannedMs)} break`;
  const startLabel = scheduled
    ? `Start ${reminder.label.toLowerCase()}`
    : "Start break";
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
          className="rounded-lg bg-accent px-3 py-1.5 font-semibold text-[12px] text-accent-fg transition-opacity hover:opacity-90"
        >
          {startLabel}
        </button>
        {reminder.canSnooze &&
          (scheduled ? (
            <button
              type="button"
              onClick={() => act(api.snoozeBreak(15))}
              className="rounded-lg border border-line bg-surface px-3 py-1.5 font-medium text-[12px] text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
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
                <Menu align="left" menuRef={menuRef}>
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
          className="rounded-lg px-2.5 py-1.5 font-medium text-[12px] text-fg-soft transition-colors hover:bg-surface-strong hover:text-fg"
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
            <Menu align="right" menuRef={menuRef}>
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
    <div className="flex overflow-hidden rounded-lg border border-line bg-surface">
      <button
        type="button"
        onClick={() => act(api.snoozeBreak(minutes))}
        className="px-3 py-1.5 font-medium text-[12px] text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
      >
        Snooze {minutes}m
      </button>
      <button
        type="button"
        aria-label="Choose snooze length"
        aria-expanded={open}
        onClick={onToggle}
        className="grid w-7 place-items-center border-line border-l text-fg-soft transition-colors hover:bg-surface-strong hover:text-fg"
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
  menuRef,
  children,
}: {
  align: "left" | "right";
  menuRef: RefObject<HTMLDivElement | null>;
  children: ReactNode;
}) {
  return (
    <div
      ref={menuRef}
      className={`absolute top-full z-50 mt-2 flex w-fit min-w-[150px] flex-col rounded-lg border border-line bg-surface p-1 ${
        align === "right" ? "right-0" : "left-0"
      }`}
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
      className="rounded-md px-2.5 py-1.5 text-left text-[12px] text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg"
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

/** The countdown capsule; hovering opens the controls. */
function BreakCard({
  current,
  now,
  expanded,
}: {
  current: BreakView;
  now: number;
  expanded: boolean;
}) {
  const end = current.startedAt + current.plannedMs;
  const remaining = end - now;
  const over = remaining <= 0;
  const progress = Math.min(
    1,
    Math.max(0, (now - current.startedAt) / current.plannedMs),
  );
  return (
    <div className={expanded ? "w-[260px]" : "w-[190px]"}>
      <div className="flex h-[34px] items-center gap-2 px-3.5">
        <span
          className={`size-2 shrink-0 rounded-full bg-break ${over ? "" : "animate-pulse"}`}
        />
        <span className="min-w-0 flex-1 truncate font-semibold text-[12px] text-fg-strong">
          {over ? `${current.label} over` : current.label}
        </span>
        <span className="font-semibold text-[12px] text-break tabular-nums">
          {over ? "0:00" : formatCountdown(remaining)}
        </span>
      </div>
      <div className="h-[2px] bg-line">
        <div
          className="h-full bg-break transition-[width] duration-1000 ease-linear"
          style={{ width: `${progress * 100}%` }}
        />
      </div>
      {expanded && (
        <div className="px-3.5 py-2.5">
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
