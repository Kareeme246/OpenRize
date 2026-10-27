import { type ReactNode, useEffect, useState } from "react";
import { useAccounts } from "../hooks/useAccounts";
import {
  type DataSummary,
  formatDataSummary,
  MAX_NAME_LENGTH,
} from "../lib/accounts";
import * as api from "../lib/api";
import { THIS_COMPUTER } from "../lib/platform";
import { FIELD } from "./Page";
import { Field, Sheet } from "./Sheet";

const NAME_HINT = `Just a name to tell accounts apart. There is no password: anyone using ${THIS_COMPUTER} can switch to any account.`;

/**
 * Adds an account and opens it (the app restarts). From No account with data
 * of its own, also asks whether that data moves into the new account.
 */
export function AddAccountSheet({ onClose }: { onClose: () => void }) {
  const { accounts, create } = useAccounts();
  const [name, setName] = useState("");
  // Undefined until No account's data has been counted.
  const [existing, setExisting] = useState<DataSummary | null | undefined>();
  const [move, setMove] = useState(true);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .existingDataSummary()
      .then(setExisting)
      .catch(() => setExisting(null));
  }, []);

  const trimmed = name.trim();
  const taken = (accounts ?? []).some(
    (account) => account.name.toLowerCase() === trimmed.toLowerCase(),
  );
  const moving = existing != null && move;
  const shown = trimmed || "the new account";
  const subject = trimmed || "The new account";

  const submit = async (): Promise<void> => {
    setBusy(true);
    setFailure(null);
    const why = await create(trimmed, moving);
    if (why === null) onClose();
    else {
      setFailure(why);
      setBusy(false);
    }
  };

  return (
    <Sheet
      title="Add an account"
      submitLabel={moving ? "Add and move data" : "Add account"}
      width="sm"
      onSubmit={() => void submit()}
      onClose={onClose}
      canSubmit={trimmed !== "" && !taken && existing !== undefined && !busy}
      error={taken ? `There is already an account called ${trimmed}.` : failure}
    >
      <Field label="Name" htmlFor="account-name" hint={NAME_HINT}>
        <input
          id="account-name"
          className={FIELD}
          value={name}
          maxLength={MAX_NAME_LENGTH}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setName(event.target.value)}
        />
      </Field>
      {existing != null && (
        <fieldset>
          <legend className="mb-1 block font-medium text-[11.5px] text-fg-soft">
            Your existing data
          </legend>
          <p className="mb-2 text-[11.5px] leading-5 text-fg-muted">
            {formatDataSummary(existing)}
          </p>
          <div className="flex flex-col gap-2">
            <Choice
              checked={move}
              onSelect={() => setMove(true)}
              title={`Move it into ${shown}`}
            >
              {`${subject} opens with everything you have now, including running stopwatches. No account starts empty.`}
            </Choice>
            <Choice
              checked={!move}
              onSelect={() => setMove(false)}
              title="Leave it with no account"
            >
              {`${subject} starts empty. Your existing data stays where it is; choose No account to get back to it.`}
            </Choice>
          </div>
        </fieldset>
      )}
      <p className="text-[11px] text-fg-faint">
        {moving
          ? "OpenRize restarts to open the account. Nothing is copied or deleted: the data folder is moved."
          : "OpenRize restarts to open the account. Running stopwatches pause first."}
      </p>
    </Sheet>
  );
}

function Choice({
  checked,
  onSelect,
  title,
  children,
}: {
  checked: boolean;
  onSelect: () => void;
  title: string;
  children: ReactNode;
}) {
  return (
    <label
      className={`flex cursor-pointer gap-2.5 rounded-lg border px-3 py-2.5 transition-colors ${
        checked
          ? "border-accent/60 bg-accent-soft"
          : "border-line bg-surface hover:bg-surface-strong"
      }`}
    >
      <input
        type="radio"
        name="existing-data"
        checked={checked}
        onChange={onSelect}
        className="mt-0.5 accent-(--accent)"
      />
      <span className="min-w-0">
        <span className="block font-semibold text-[12.5px] text-fg-strong">
          {title}
        </span>
        <span className="mt-0.5 block text-[11.5px] leading-5 text-fg-muted">
          {children}
        </span>
      </span>
    </label>
  );
}
