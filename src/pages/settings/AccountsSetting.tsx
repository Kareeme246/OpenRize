import { useState } from "react";
import { AccountTile } from "../../components/AccountSwitcher";
import { AddAccountSheet } from "../../components/AddAccountSheet";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { FIELD } from "../../components/Page";
import { Field, Sheet } from "../../components/Sheet";
import { Tooltip } from "../../components/Tooltip";
import { useAccounts } from "../../hooks/useAccounts";
import {
  type AccountView,
  formatBytes,
  MAX_NAME_LENGTH,
} from "../../lib/accounts";
import * as api from "../../lib/api";
import { describeError } from "../../lib/api";
import { THIS_COMPUTER } from "../../lib/platform";
import { SettingGroup } from "./SettingParts";

const ROW_BUTTON =
  "rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] text-fg-muted hover:bg-surface-strong disabled:cursor-not-allowed disabled:opacity-40";

/** Settings > Data & storage > Accounts: the list, with rename and delete. */
export function AccountsSetting() {
  const { accounts, current, readOnly } = useAccounts();
  const [adding, setAdding] = useState(false);
  const [renaming, setRenaming] = useState<AccountView | null>(null);
  const [deleting, setDeleting] = useState<AccountView | null>(null);
  const [error, setError] = useState<string | null>(null);

  const remove = (account: AccountView): void => {
    setDeleting(null);
    setError(null);
    api
      .deleteAccount(account.id)
      .catch((cause: unknown) => setError(describeError(cause)));
  };

  const listed = accounts ?? [];
  return (
    <SettingGroup title="Accounts">
      <div className="flex flex-col divide-y divide-line px-4">
        {listed.length === 0 && (
          <p className="py-3 text-[12px] leading-relaxed text-fg-muted">
            No accounts yet. An account is a name with its own time entries,
            projects, clients, invoices and stopwatches. There is no password:
            anyone using {THIS_COMPUTER} can switch to any account.
          </p>
        )}
        {listed.map((account) => {
          const open = account.id === current?.id;
          return (
            <div key={account.id} className="flex items-center gap-3 py-2.5">
              <AccountTile account={account} />
              <div className="min-w-0 flex-1">
                <p className="truncate font-semibold text-[13px] text-fg-strong">
                  {account.name}
                  {open && (
                    <span className="ml-2 rounded-full bg-accent-soft px-2 py-0.5 font-semibold text-[10.5px] text-accent">
                      Open now
                    </span>
                  )}
                </p>
                <p
                  className={`truncate text-[11px] ${
                    account.missing ? "text-review" : "text-fg-soft"
                  }`}
                >
                  {account.missing
                    ? "Data folder missing - it was moved or deleted outside OpenRize"
                    : `Added ${new Date(account.createdAt).toLocaleDateString(
                        undefined,
                        { month: "short", day: "numeric", year: "numeric" },
                      )} · ${formatBytes(account.sizeBytes)}`}
                </p>
              </div>
              {!account.missing && (
                <button
                  type="button"
                  className={ROW_BUTTON}
                  disabled={readOnly}
                  onClick={() => setRenaming(account)}
                >
                  Rename...
                </button>
              )}
              <Tooltip
                content={
                  open
                    ? "Switch to another account first"
                    : account.missing
                      ? "Forget this account"
                      : "Move this account's data to the Trash"
                }
              >
                <button
                  type="button"
                  className={`${ROW_BUTTON} text-danger`}
                  disabled={open || readOnly}
                  onClick={() =>
                    account.missing ? remove(account) : setDeleting(account)
                  }
                >
                  {account.missing ? "Remove" : "Delete..."}
                </button>
              </Tooltip>
            </div>
          );
        })}
        <div className="flex items-center gap-3 py-2.5">
          <button
            type="button"
            className={ROW_BUTTON}
            disabled={readOnly}
            onClick={() => setAdding(true)}
          >
            Add account...
          </button>
          {readOnly && (
            <span className="text-[11px] text-fg-soft">
              These accounts were made by a newer version of OpenRize. Update to
              change them.
            </span>
          )}
        </div>
        {error && (
          <p role="alert" className="py-2.5 text-[11.5px] text-danger">
            {error}
          </p>
        )}
      </div>
      {adding && <AddAccountSheet onClose={() => setAdding(false)} />}
      {renaming && (
        <RenameSheet account={renaming} onClose={() => setRenaming(null)} />
      )}
      {deleting && (
        <ConfirmDialog
          title={`Delete ${deleting.name}?`}
          body={`${deleting.name}'s time entries, projects, clients, invoices and stopwatches move to the Trash as one folder. You can put it back from the Trash until you empty it.`}
          confirmLabel={`Delete ${deleting.name}`}
          onCancel={() => setDeleting(null)}
          onConfirm={() => remove(deleting)}
        />
      )}
    </SettingGroup>
  );
}

function RenameSheet({
  account,
  onClose,
}: {
  account: AccountView;
  onClose: () => void;
}) {
  const { accounts } = useAccounts();
  const [name, setName] = useState(account.name);
  const [failure, setFailure] = useState<string | null>(null);
  const trimmed = name.trim();
  const taken = (accounts ?? []).some(
    (other) =>
      other.id !== account.id &&
      other.name.toLowerCase() === trimmed.toLowerCase(),
  );

  return (
    <Sheet
      title={`Rename ${account.name}`}
      submitLabel="Rename"
      width="sm"
      onClose={onClose}
      canSubmit={trimmed !== "" && !taken && trimmed !== account.name}
      error={taken ? `There is already an account called ${trimmed}.` : failure}
      onSubmit={() => {
        api
          .renameAccount(account.id, trimmed)
          .then(onClose)
          .catch((cause: unknown) => setFailure(describeError(cause)));
      }}
    >
      <Field label="Name" htmlFor="rename-account">
        <input
          id="rename-account"
          className={FIELD}
          value={name}
          maxLength={MAX_NAME_LENGTH}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setName(event.target.value)}
        />
      </Field>
    </Sheet>
  );
}
