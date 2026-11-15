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
