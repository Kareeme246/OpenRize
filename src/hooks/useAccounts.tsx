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

const RESTART_PATIENCE_MS = 20_000;

/** Where a switch is headed; a null name is No account (Sign out). */
export interface Switching {
  name: string | null;
}

export interface AccountsApi {
  accounts: AccountView[] | null;
  current: AccountView | null;
  missing: Account | null;
  readOnly: boolean;
  switching: Switching | null;
  switchTo: (account: Account | null) => Promise<void>;
  /**
   * Resolves to why it could not (a name already taken, say), or null once
   * under way.
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
 * One reader of the accounts for the window; the list changes only by a rename
 * or delete, via `accounts-changed`.
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
