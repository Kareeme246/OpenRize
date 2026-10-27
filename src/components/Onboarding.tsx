import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import { useRelaunch } from "../hooks/useRelaunch";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { useToast } from "../hooks/useToast";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import {
  accessibilityCard,
  automationCard,
  type OnboardingStep,
  onboardingAction,
  type PermissionCardView,
} from "../lib/onboarding";
import { ACCESSIBILITY_ID, roleLabel, TONE_CLASS } from "../lib/permissions";
import type { PermissionInfo } from "../lib/types";

const AUTOMATION_ID = "automation";

/**
 * The first screen a Mac user sees: the permissions OpenRize can use, each
 * explained once. Accessibility is recommended and Automation optional, and
 * neither gates Continue, so skipping is the normal path. When a grant needs
 * the app restarted, the same button becomes Restart. See `onboardingStep` for
 * when it comes up and `useOnboarding` for the state behind it.
 */
export function Onboarding({
  step,
  onFinish,
}: {
  step: OnboardingStep;
  onFinish: () => void;
}) {
  if (step === "hidden") return null;
  return <Screen step={step} onFinish={onFinish} />;
}

function Screen({
  step,
  onFinish,
}: {
  step: Exclude<OnboardingStep, "hidden">;
  onFinish: () => void;
}) {
  const showToast = useToast();
  // Marks onboarding done first, so the restarted app does not show it again.
  const { relaunching, failed, relaunch } = useRelaunch(api.finishOnboarding);
  const action = onboardingAction(step);
  const [permissions, setPermissions] = useState<PermissionInfo[]>([]);
  const load = useCallback(() => {
    api
      .checkPermissions()
      .then(setPermissions)
      .catch((cause: unknown) =>
        console.error("Could not read permissions", describeError(cause)),
      );
  }, []);
  useEffect(load, [load]);
  useTauriEvent(api.PERMISSIONS_CHANGED, load);

  const urlOf = (id: string): string | undefined =>
    permissions.find((permission) => permission.id === id)?.settingsUrl;
  const descriptionOf = (id: string): string | undefined =>
    permissions.find((permission) => permission.id === id)?.description;
  const open = (url: string | undefined): void => {
    if (url === undefined) return;
    openUrl(url).catch((cause: unknown) =>
      showToast({ tone: "error", message: describeError(cause) }),
    );
  };

  const allow = (): void => {
    api
      .requestAccessibility()
      .then(() => open(urlOf(ACCESSIBILITY_ID)))
      .catch((cause: unknown) =>
        showToast({ tone: "error", message: describeError(cause) }),
      );
  };

  const automation = permissions.find(
    (permission) => permission.id === AUTOMATION_ID,
  );

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center overflow-y-auto bg-canvas p-4">
      <section
        aria-labelledby="onboarding-title"
        className="flex w-[480px] max-w-full flex-col gap-4 rounded-2xl border border-line bg-panel p-6 shadow-2xl"
      >
        <header className="space-y-1.5">
          <h1
            id="onboarding-title"
            className="text-[16px] font-semibold text-fg-strong"
          >
            Permissions
          </h1>
          <p className="text-[12.5px] leading-relaxed text-fg-soft">
            You can change these later in Settings.
          </p>
        </header>

        <PermissionRow
          title="Accessibility"
          role={roleLabel(ACCESSIBILITY_ID, false)}
          detail={descriptionOf(ACCESSIBILITY_ID)}
          view={accessibilityCard(step)}
          onGrant={allow}
        />
        <PermissionRow
          title="Automation"
          role={roleLabel(AUTOMATION_ID, false)}
          detail={descriptionOf(AUTOMATION_ID)}
          view={automationCard(automation?.granted)}
          onGrant={() => open(urlOf(AUTOMATION_ID))}
        />

        <div className="flex flex-col items-end gap-2 border-t border-line/60 pt-4">
          {action.kind === "restart" && (
            <p className="text-[12.5px] text-amber-500">{action.note}</p>
          )}
          {failed && (
            <p className="text-[12px] text-rose-500" role="alert">
              OpenRize couldn't restart itself. Quit it from the menu bar and
              open it again.
            </p>
          )}
          <button
            type="button"
            onClick={action.kind === "restart" ? relaunch : onFinish}
            disabled={relaunching || action.disabled}
            className="shrink-0 rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3.5 py-1.5 text-[12.5px] font-semibold text-accent-fg disabled:opacity-60"
          >
            {relaunching ? "Restarting…" : action.label}
          </button>
        </div>
      </section>
    </div>
  );
}

function PermissionRow({
  title,
  role,
  detail,
  view,
  onGrant,
}: {
  title: string;
  role: string;
  detail: string | undefined;
  view: PermissionCardView | null;
  onGrant: () => void;
}) {
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-settings-card-border bg-settings-card p-4">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[13px] font-medium text-fg">{title}</span>
        <span className="shrink-0 rounded-full border border-line bg-surface px-2 py-0.5 font-mono text-[9.5px] uppercase tracking-wider text-fg-muted">
          {role}
        </span>
      </div>
      {detail !== undefined && (
        <p className="text-[12px] leading-relaxed text-fg-muted">{detail}</p>
      )}
      {view !== null && (
        <div className="flex items-center justify-between gap-2">
          <span
            className={`min-w-0 text-[12px] font-medium ${TONE_CLASS[view.tone]}`}
          >
            {view.status}
          </span>
          {view.showGrant && (
            <button
              type="button"
              onClick={onGrant}
              aria-label={`Grant ${title}`}
              className="shrink-0 rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3 py-1.5 text-[12px] font-semibold text-accent-fg"
            >
              Grant
            </button>
          )}
        </div>
      )}
    </div>
  );
}
