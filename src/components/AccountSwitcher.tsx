import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useRef,
  useState,
} from "react";
import { useAccounts } from "../hooks/useAccounts";
import { useDismiss } from "../hooks/useDismiss";
import { useToast } from "../hooks/useToast";
import {
  type Account,
  accountColor,
  describePausedWork,
  type SwitchPreview,
} from "../lib/accounts";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import type { Route } from "../lib/types";
import { AddAccountSheet } from "./AddAccountSheet";
import { ConfirmDialog } from "./ConfirmDialog";
import { BUTTON_PRIMARY } from "./Page";
import { Tooltip } from "./Tooltip";

/** A square tile: the account's first letter on its colour, or a person for No account. */
export function AccountTile({
  account,
  size = 26,
}: {
  account: Account | null;
  size?: number;
}) {
  const roundedClass = size <= 20 ? "rounded-[5px]" : "rounded-[7px]";
  if (account === null) {
    return (
      <span
        aria-hidden="true"
        className={`flex shrink-0 items-center justify-center ${roundedClass} bg-surface-strong text-fg-muted`}
        style={{ width: size, height: size }}
      >
        <svg
          aria-hidden="true"
          viewBox="0 0 24 24"
          className={size <= 20 ? "size-3" : "size-3.5"}
          fill="none"
          stroke="currentColor"
          strokeWidth={2}
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <path d="M5 21v-2a4 4 0 0 1 4-4h6a4 4 0 0 1 4 4v2" />
          <circle cx="12" cy="7" r="4" />
        </svg>
      </span>
    );
  }
  return (
    <span
      aria-hidden="true"
      className={`flex shrink-0 items-center justify-center ${roundedClass} font-extrabold text-cat-ink`}
      style={{
        width: size,
        height: size,
        fontSize: size * 0.46,
        background: accountColor(account.id),
      }}
    >
      {account.name.trim().charAt(0).toUpperCase() || "?"}
    </span>
  );
}

const MENU_ITEM =
  "flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[12.5px] text-fg transition-colors hover:bg-surface focus-visible:bg-surface focus-visible:outline-hidden disabled:cursor-not-allowed disabled:opacity-40";

function MenuItem({
  icon,
  danger,
  onSelect,
  children,
}: {
  icon: ReactNode;
  danger?: boolean;
  onSelect: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onSelect}
      className={`${MENU_ITEM} ${danger ? "text-danger" : ""}`}
    >
      <svg
        viewBox="0 0 24 24"
        className={`size-[15px] shrink-0 ${danger ? "" : "text-fg-soft"}`}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.8}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {icon}
      </svg>
      <span className="min-w-0 flex-1 truncate">{children}</span>
    </button>
  );
}

function AccountRow({
  account,
  current,
  onSelect,
}: {
  account: Account;
  current: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={current}
      onClick={onSelect}
      className={`${MENU_ITEM} ${current ? "bg-surface" : ""}`}
    >
      <AccountTile account={account} size={22} />
      <span className="min-w-0 flex-1 truncate">{account.name}</span>
      {current && (
        <span aria-hidden="true" className="font-extrabold text-accent">
          {"✓"}
        </span>
      )}
    </button>
  );
}

const Separator = () => <div className="mx-1 my-1.5 h-px bg-line" />;

/** A switch waiting on the user to agree that running work pauses. */
interface PendingSwitch {
  to: Account | null;
  preview: SwitchPreview;
}

interface AccountSwitcherProps {
  onNavigate: (route: Route) => void;
  collapsed?: boolean;
  variant?: "bar" | "rail";
}

/**
 * The account switcher in the bar (or rail). Choosing another account (or
 * Sign out) restarts the app into it; running work pauses first and waits for
 * the user in the account it belongs to.
 */
export function AccountSwitcher({
  onNavigate,
  collapsed = false,
  variant = "bar",
}: AccountSwitcherProps) {
  const { accounts, current, readOnly, switchTo } = useAccounts();
  const showToast = useToast();
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const [pending, setPending] = useState<PendingSwitch | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(open, root, close);

  const title = current?.name ?? "No account";
  const subtitle = current ? "Local account" : "Add an account";
  const listed = accounts ?? [];

  /** Asks for agreement first when anything is running, then switches. */
  const requestSwitch = async (to: Account | null): Promise<void> => {
    close();
    let preview: SwitchPreview;
    try {
      preview = await api.switchPreview();
    } catch (cause) {
      showToast({ tone: "error", message: describeError(cause) });
      return;
    }
    if (preview.stopwatches.length === 0 && preview.break === null) {
      await switchTo(to);
      return;
    }
    setPending({ to, preview });
  };

  const moveFocus = (event: KeyboardEvent<HTMLDivElement>): void => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    const items = Array.from(
      menu.current?.querySelectorAll<HTMLElement>(
        "[role^='menuitem'], button",
      ) ?? [],
    );
    if (items.length === 0) return;
    event.preventDefault();
    const index = items.indexOf(document.activeElement as HTMLElement);
    const step = event.key === "ArrowDown" ? 1 : -1;
    items[(index + step + items.length) % items.length]?.focus();
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the arrow keys move between the menu's own buttons, from the trigger too
    <div ref={root} onKeyDown={moveFocus} className="relative shrink-0">
      <Tooltip
        content={`${title} - switch account`}
        placement={variant === "bar" ? "bottom" : "right"}
      >
        <button
          type="button"
          aria-haspopup="menu"
          aria-expanded={open}
          aria-label={`Account: ${title}`}
          onClick={() => setOpen((was) => !was)}
          className={
            variant === "bar"
              ? `flex h-7 w-40 items-center gap-1.5 rounded-lg border border-line px-2 text-left transition-colors hover:bg-surface-strong ${
                  open ? "border-line-strong bg-surface-strong" : "bg-surface"
                }`
              : `flex w-full items-center gap-2.5 rounded-[10px] border border-line px-2 py-1.5 text-left transition-colors hover:bg-surface-strong ${
                  open ? "border-line-strong bg-surface-strong" : "bg-surface"
                } ${collapsed ? "justify-center px-1" : ""}`
          }
        >
          <AccountTile account={current} size={variant === "bar" ? 20 : 26} />
          {variant === "bar" ? (
            <>
              <span className="min-w-0 flex-1 truncate font-medium text-[12px] text-fg-strong">
                {title}
              </span>
              <span
                aria-hidden="true"
                className="shrink-0 text-[10px] text-fg-soft"
              >
                {open ? "▴" : "▾"}
              </span>
            </>
          ) : (
            !collapsed && (
              <>
                <span className="min-w-0 flex-1 leading-tight">
                  <span className="block truncate font-semibold text-[13px] text-fg-strong">
                    {title}
                  </span>
                  <span className="block truncate text-[10.5px] text-fg-soft">
                    {subtitle}
                  </span>
                </span>
                <span aria-hidden="true" className="text-[10px] text-fg-soft">
                  {open ? "▴" : "▾"}
                </span>
              </>
            )
          )}
        </button>
      </Tooltip>

      {open && (
        <div
          ref={menu}
          role="menu"
          aria-label="Account"
          data-tauri-drag-region="none"
          className={`absolute z-50 w-[272px] rounded-xl border border-line-strong bg-panel p-1.5 shadow-2xl ${
            variant === "bar"
              ? "top-full left-0 mt-1.5"
              : collapsed
                ? "top-0 left-full ml-2"
                : "top-full left-0 mt-1.5"
          }`}
        >
          {listed.length === 0 ? (
            <div className="px-2.5 py-2">
              <p className="font-semibold text-[12.5px] text-fg-strong">
                Keep separate data with accounts
              </p>
              <p className="mt-1.5 text-[11.5px] leading-5 text-fg-muted">
                An account is just a name with its own time entries, projects,
                clients, invoices and stopwatches. There is no password: anyone
                using this Mac can switch to any account.
              </p>
              <div className="mt-3 flex gap-2">
                <button
                  type="button"
                  className={BUTTON_PRIMARY}
                  disabled={readOnly}
                  onClick={() => {
                    close();
                    setAdding(true);
                  }}
                >
                  Add account
                </button>
              </div>
            </div>
          ) : (
            <>
              <p className="px-2.5 pt-2 pb-1 font-bold text-[10.5px] text-fg-faint uppercase tracking-wider">
                Accounts
              </p>
              {listed.map((account) => (
                <AccountRow
                  key={account.id}
                  account={account}
                  current={account.id === current?.id}
                  onSelect={() => {
                    if (account.id === current?.id) close();
                    else void requestSwitch(account);
                  }}
                />
              ))}
              <Separator />
              <MenuItem
                icon={<path d="M12 5v14M5 12h14" />}
                onSelect={() => {
                  close();
                  setAdding(true);
                }}
              >
                Add account...
              </MenuItem>
              <MenuItem
                icon={
                  <>
                    <path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1" />
                    <circle cx="15" cy="6" r="2" />
                    <circle cx="9" cy="12" r="2" />
                    <circle cx="17" cy="18" r="2" />
                  </>
                }
                onSelect={() => {
                  close();
                  onNavigate({ name: "settings", section: "data" });
                }}
              >
                Manage accounts...
              </MenuItem>
              {current && (
                <>
                  <Separator />
                  <MenuItem
                    danger
                    icon={
                      <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4M16 17l5-5-5-5M21 12H9" />
                    }
                    onSelect={() => void requestSwitch(null)}
                  >
                    Sign out
                  </MenuItem>
                </>
              )}
            </>
          )}
        </div>
      )}

      {adding && <AddAccountSheet onClose={() => setAdding(false)} />}
      {pending && (
        <ConfirmDialog
          tone="affirmative"
          title={
            pending.to
              ? `Switch to ${pending.to.name}?`
              : `Sign out of ${current?.name ?? "this account"}?`
          }
          body={pauseNotice(pending, current)}
          confirmLabel={pending.to ? "Pause and switch" : "Pause and sign out"}
          onCancel={() => setPending(null)}
          onConfirm={() => {
            const { to } = pending;
            setPending(null);
            void switchTo(to);
          }}
        />
      )}
    </div>
  );
}

/** What pauses, where it stays, and where OpenRize reopens. */
function pauseNotice(pending: PendingSwitch, from: Account | null): string {
  const { stopwatches, break: running } = pending.preview;
  const several = stopwatches.length + (running ? 1 : 0) > 1;
  const what = describePausedWork(stopwatches, running);
  const staysWith = from ? `with ${from.name}` : "in No account";
  const reopens = pending.to ? `on ${pending.to.name}` : "with no account";
  return `${what} ${several ? "pause now and stay" : "pauses now and stays"} ${staysWith}. ${
    several ? "Resume them" : "Resume it"
  } when you come back. OpenRize reopens ${reopens} in a moment.`;
}
