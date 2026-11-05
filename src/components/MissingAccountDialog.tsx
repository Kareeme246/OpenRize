import { useAccounts } from "../hooks/useAccounts";
import { useToast } from "../hooks/useToast";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { ConfirmDialog } from "./ConfirmDialog";

/**
 * Shown once when the remembered account lost its data folder. Cancel keeps it
 * listed; the other button forgets it.
 */
export function MissingAccountDialog() {
  const { missing, reload } = useAccounts();
  const showToast = useToast();
  if (missing === null) return null;

  const dismiss = (): void => {
    api
      .dismissMissingAccount()
      .catch((cause: unknown) =>
        showToast({ tone: "error", message: describeError(cause) }),
      );
  };
  const forget = (): void => {
    api
      .deleteAccount(missing.id)
      .then(dismiss)
      .catch((cause: unknown) => {
        showToast({ tone: "error", message: describeError(cause) });
        reload();
      });
  };

  return (
    <ConfirmDialog
      title={`${missing.name}'s data is missing`}
      body={`OpenRize couldn't find ${missing.name}'s data folder, so it opened with no account. If you moved it, put it back in OpenRize's accounts folder and switch to ${missing.name} again. Nothing else was changed.`}
      confirmLabel={`Remove ${missing.name} from the list`}
      onCancel={dismiss}
      onConfirm={forget}
    />
  );
}
