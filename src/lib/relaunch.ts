/**
 * Relaunches the app, after an optional step that must not stand in its way.
 * The relaunch screen and the first-run Restart button both go through this,
 * with `api.relaunchApp` as `relaunch`, so there is one way the app restarts
 * for a permission. A `prepare` that fails is reported and the relaunch goes
 * ahead: the restart is what the user needs.
 */
export async function relaunchAfter(
  prepare: (() => Promise<void>) | undefined,
  relaunch: () => Promise<void>,
  onPrepareError: (cause: unknown) => void,
): Promise<void> {
  if (prepare !== undefined) {
    try {
      await prepare();
    } catch (cause) {
      onPrepareError(cause);
    }
  }
  await relaunch();
}
