import { openUrl } from "@tauri-apps/plugin-opener";
import { type ReactNode, useCallback, useEffect, useState } from "react";
import {
  SegmentedControl,
  type SegmentedOption,
} from "../../components/SegmentedControl";
import { useTauriEvent } from "../../hooks/useTauriEvent";
import { useToast } from "../../hooks/useToast";
import * as api from "../../lib/api";
import { describeError } from "../../lib/api";
import {
  type AccessibilityChoice,
  type AutomationChoice,
  accessibilityChoice,
  accessibilityFromChoice,
  automationChoice,
  automationFromChoice,
  isReleaseIdentifier,
  tccResetCommand,
} from "../../lib/devTools";
import type { DevToolsInfo } from "../../lib/types";

const ACCESSIBILITY_OPTIONS: SegmentedOption<AccessibilityChoice>[] = [
  { value: "real", label: "Real" },
  { value: "notGranted", label: "Off" },
  { value: "checking", label: "Checking" },
  { value: "notWorking", label: "Not working" },
  { value: "relaunchRequired", label: "Relaunch" },
  { value: "working", label: "On" },
];

const AUTOMATION_OPTIONS: SegmentedOption<AutomationChoice>[] = [
  { value: "real", label: "Real" },
  { value: "missing", label: "Off" },
  { value: "allowed", label: "On" },
];

const ACCESSIBILITY_SETTINGS =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

const buttonClass =
  "shrink-0 cursor-pointer rounded-md border border-line bg-surface px-2.5 py-1 text-[11.5px] font-medium text-fg-muted transition-colors hover:bg-surface-strong hover:text-fg";

/**
 * Developer-only controls for testing permissions and onboarding by hand.
 * Rendered only in a macOS dev build (see `DEV_TOOLS`), and only answers when the
 * app runs under the dev identifier. Every status comes from re-reading
 * `dev_tools_info`, so the panel always shows what the backend holds.
 */
export function DevToolsSetting() {
  const showToast = useToast();
  const [info, setInfo] = useState<DevToolsInfo | null>(null);
  const [unavailable, setUnavailable] = useState(false);

  const load = useCallback(() => {
    api
      .devToolsInfo()
      .then((loaded) => {
        setInfo(loaded);
        setUnavailable(false);
      })
      .catch(() => {
        setInfo(null);
        setUnavailable(true);
      });
  }, []);
  useEffect(load, [load]);
  useTauriEvent(api.PERMISSIONS_CHANGED, load);
  useTauriEvent(api.ONBOARDING_CHANGED, load);

  /** Runs a dev command, then re-reads what it changed. */
  const run = (action: () => Promise<void>): void => {
    action()
      .then(load)
      .catch((cause: unknown) =>
        showToast({ tone: "error", message: describeError(cause) }),
      );
  };

  if (unavailable) {
    return (
      <p className="text-[12px] leading-relaxed text-fg-muted">
        Developer tools need the dev identifier. Start the app with pnpm tauri
        dev.
      </p>
    );
  }
  if (info === null) {
    return <p className="text-[12px] text-fg-muted">Loading…</p>;
  }

  const command = tccResetCommand(info.identifier);
  const isRelease = isReleaseIdentifier(
    info.identifier,
    info.releaseIdentifier,
  );

  const copyCommand = (): void => {
    navigator.clipboard
      .writeText(command)
      .then(() => showToast({ tone: "success", message: "Copied" }))
      .catch((cause: unknown) =>
        showToast({ tone: "error", message: describeError(cause) }),
      );
  };

  const openAccessibility = (): void => {
    openUrl(ACCESSIBILITY_SETTINGS).catch((cause: unknown) =>
      showToast({ tone: "error", message: describeError(cause) }),
    );
  };

  return (
    <div className="flex flex-col gap-4 rounded-xl border border-dashed border-amber-500/50 bg-amber-500/5 p-4">
      <div className="flex flex-col gap-1.5">
        <div className="flex items-center gap-2">
          <span className="shrink-0 rounded-full border border-amber-500/40 px-2 py-0.5 font-mono text-[9.5px] uppercase tracking-wider text-amber-500">
            Dev build only
          </span>
          <span className="text-[13px] font-medium text-fg-strong">
            Developer tools
          </span>
        </div>
        <p className="text-[12px] leading-relaxed text-fg-muted">
          Simulates permission and onboarding states for testing. Nothing here
          changes macOS privacy settings or your data.
        </p>
      </div>

      <DevBlock
        title="Simulate permissions"
        description="Makes this running app report a permission state instead of the real one. macOS is not asked or changed, and the simulation ends on relaunch or when you press Allow Accessibility. Relaunch shows the relaunch screen, which only a relaunch clears."
      >
        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <span className="text-[12px] text-fg-muted">Accessibility</span>
            <SegmentedControl
              name="dev-accessibility"
              value={accessibilityChoice(info.accessibility)}
              options={ACCESSIBILITY_OPTIONS}
              onChange={(choice) =>
                run(() =>
                  api.devSimulateAccessibility(accessibilityFromChoice(choice)),
                )
              }
            />
          </div>
          <div className="flex flex-wrap items-center justify-between gap-3">
            <span className="text-[12px] text-fg-muted">Automation</span>
            <SegmentedControl
              name="dev-automation"
              value={automationChoice(info.automation)}
              options={AUTOMATION_OPTIONS}
              onChange={(choice) =>
                run(() =>
                  api.devSimulateAutomation(automationFromChoice(choice)),
                )
              }
            />
          </div>
        </div>
      </DevBlock>

      <DevBlock
        title="Re-run onboarding"
        description="Opens the first-run Permissions screen now, simulating Accessibility as off if it is on. The saved 'already seen' marker is kept, because the installed app shares it."
      >
        <div>
          <button
            type="button"
            onClick={() => run(() => api.devRerunOnboarding())}
            className={buttonClass}
          >
            Show onboarding
          </button>
        </div>
      </DevBlock>

      <DevBlock
        title="Reset"
        description="Clears every simulation above and any onboarding re-run, so this running app reports the real states again."
      >
        <div className="flex flex-col gap-4">
          <div>
            <button
              type="button"
              onClick={() => run(() => api.devReset())}
              className={buttonClass}
            >
              Reset simulations
            </button>
          </div>

          <div className="flex flex-col gap-3 border-t border-line/60 pt-4">
            <div className="text-[12px] font-medium text-fg">
              Real Accessibility grant
            </div>
            <p className="text-[12px] leading-relaxed text-fg-muted">
              macOS keeps the real grant, per app identity. This dev build runs
              as {info.identifier}, not the installed app's{" "}
              {info.releaseIdentifier}. To revoke it for real, run the command
              below, or remove the entry in System Settings. A dev build
              launched from a terminal may be listed under the terminal instead.
            </p>
            {!isRelease && (
              <div className="flex items-center gap-2">
                <code className="min-w-0 flex-1 rounded-md border border-line bg-surface px-2 py-1 font-mono text-[11.5px] break-all select-all">
                  {command}
                </code>
                <button
                  type="button"
                  onClick={copyCommand}
                  className={buttonClass}
                >
                  Copy
                </button>
              </div>
            )}
            <div>
              <button
                type="button"
                onClick={openAccessibility}
                className={buttonClass}
              >
                Open Accessibility settings
              </button>
            </div>
          </div>
        </div>
      </DevBlock>
    </div>
  );
}

/** One tool: its title, what it changes and does not, and its control. */
function DevBlock({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-3 border-t border-line/60 pt-4">
      <div className="space-y-1">
        <div className="text-[13px] font-medium text-fg">{title}</div>
        <p className="text-[12px] leading-relaxed text-fg-muted">
          {description}
        </p>
      </div>
      {children}
    </div>
  );
}
