import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import type { Account, AccountsState, AccountView } from "../lib/accounts";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { useTauriEvent } from "./useTauriEvent";
import { useToast } from "./useToast";

/** The switching screen is cleared if the restart has not come by then. */
const RESTART_PATIENCE_MS = 20_000;

/** Where a switch is headed; a null name is No account (Sign out). */
export interface Switching {
  name: string | null;
}

export interface AccountsApi {
  /** Null until the first answer. */
  accounts: AccountView[] | null;
  /** The open account, or null for No account (also while loading). */
  current: AccountView | null;
  /** The account this launch could not open because its data is gone. */
  missing: Account | null;
  /** A newer release wrote the accounts: they can be seen but not changed. */
  readOnly: boolean;
  /** Set from the moment a switch is asked for until the app restarts. */
  switching: Switching | null;
  /** Opens an account, or No account for null, by restarting the app. */
  switchTo: (account: Account | null) => Promise<void>;
  /**
   * Adds an account and opens it by restarting the app. Resolves to why it
   * could not (a name already taken, say), or null once it is under way.
   */
  create: (name: string, moveExisting: boolean) => Promise<string | null>;
  reload: () => void;
}

const AccountsContext = createContext<AccountsApi>({
  accounts: null,
  current: null,
  missing: null,
  readOnly: false,
  switching: null,
  switchTo: async () => undefined,
  create: async () => null,
  reload: () => undefined,
});

export function useAccounts(): AccountsApi {
  return useContext(AccountsContext);
}

/**
 * One reader of the accounts for the whole window. Switching is a restart, so
 * the list never changes under a live view except by a rename or a delete,
 * which arrive as `accounts-changed`.
 */
export function AccountsProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AccountsState | null>(null);
  const [switching, setSwitching] = useState<Switching | null>(null);
  const showToast = useToast();
  const patience = useRef<number>(0);

  const reload = useCallback((): void => {
    api
      .accountsState()
      .then((loaded) => {
        setState(loaded);
        // Cleared by this very read, so it is said once: never drop it.
        if (loaded.justAdopted) {
          const name =
            loaded.accounts.find((account) => account.id === loaded.current)
              ?.name ?? "the account";
          showToast({
            tone: "success",
            message: `Your data is now in ${name}. No account starts empty.`,
          });
        }
      })
      .catch((cause: unknown) =>
        console.error("Could not load accounts", describeError(cause)),
      );
  }, [showToast]);

  useEffect(reload, [reload]);
  useTauriEvent(api.ACCOUNTS_CHANGED, reload);
  useEffect(() => () => window.clearTimeout(patience.current), []);

  /** Raises the switching screen and starts the switch; null when it is under way. */
  const run = useCallback(
    async (
      name: string | null,
      start: () => Promise<unknown>,
    ): Promise<string | null> => {
      setSwitching({ name });
      try {
        await start();
        // The app restarts on its own after the minimum time.
        window.clearTimeout(patience.current);
        patience.current = window.setTimeout(() => {
          setSwitching(null);
          showToast({
            tone: "error",
            message: "OpenRize did not restart. Try switching again.",
          });
        }, RESTART_PATIENCE_MS);
        return null;
      } catch (cause) {
        setSwitching(null);
        return describeError(cause);
      }
    },
    [showToast],
  );

  const switchTo = useCallback(
    async (account: Account | null): Promise<void> => {
      const failure = await run(account?.name ?? null, () =>
        api.switchAccount(account?.id ?? null),
      );
      if (failure !== null) showToast({ tone: "error", message: failure });
    },
    [run, showToast],
  );

  const create = useCallback(
    (name: string, moveExisting: boolean) =>
      run(name.trim(), () => api.createAccount(name, moveExisting)),
    [run],
  );

  const current =
    state?.accounts.find((account) => account.id === state.current) ?? null;

  return (
    <AccountsContext.Provider
      value={{
        accounts: state?.accounts ?? null,
        current,
        missing: state?.missing ?? null,
        readOnly: state?.readOnly ?? false,
        switching,
        switchTo,
        create,
        reload,
      }}
    >
      {children}
    </AccountsContext.Provider>
  );
}
