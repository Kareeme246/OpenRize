import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useState } from "react";
import { useAccessibilityState } from "../hooks/useAccessibilityState";
import { useTauriEvent } from "../hooks/useTauriEvent";
import { useToast } from "../hooks/useToast";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { type OnboardingStep, onboardingStep } from "../lib/onboarding";
import { ACCESSIBILITY_ID } from "../lib/permissions";
import type { PermissionInfo } from "../lib/types";

const AUTOMATION_ID = "automation";

/**
 * The first screen a Mac user sees: the permissions OpenRize can use, each
 * explained once. Both are optional and skipping is the normal path, so the
 * way out is as prominent as the way in. See `onboardingStep` for when it
 * comes up and when it steps aside.
 */
export function Onboarding() {
  const accessibility = useAccessibilityState();
  const [needed, setNeeded] = useState<boolean | null>(null);
  const [shown, setShown] = useState(false);
  const step = onboardingStep({ needed, shown, accessibility });

  useEffect(() => {
    api
      .onboardingNeeded()
      .then(setNeeded)
      .catch((cause: unknown) =>
        console.error("Could not read onboarding", describeError(cause)),
      );
  }, []);
  useEffect(() => {
    if (step !== "hidden") setShown(true);
  }, [step]);

  if (step === "hidden") return null;
  return (
    <Screen
      step={step}
      onFinish={() => {
        setNeeded(false);
        api
          .finishOnboarding()
          .catch((cause: unknown) =>
            console.error("Could not save onboarding", describeError(cause)),
          );
      }}
    />
  );
}

function Screen({
  step,
  onFinish,
}: {
  step: Exclude<OnboardingStep, "hidden">;
  onFinish: () => void;
}) {
  const showToast = useToast();
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
            Both are optional. You can change them later in Settings.
          </p>
        </header>

        <PermissionRow
          title="Accessibility"
          detail={descriptionOf(ACCESSIBILITY_ID)}
          status={<AccessibilityStatus step={step} />}
          action={
            step === "asking" ? (
              <button
                type="button"
                onClick={allow}
                className="shrink-0 rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3 py-1.5 text-[12px] font-semibold text-accent-fg"
              >
                Allow Accessibility
              </button>
            ) : null
          }
        />
        <PermissionRow
          title="Automation"
          detail={descriptionOf(AUTOMATION_ID)}
          status={null}
          action={
            <button
              type="button"
              onClick={() => open(urlOf(AUTOMATION_ID))}
              className="shrink-0 rounded-lg border border-line bg-surface px-3 py-1.5 text-[12px] font-medium text-fg-muted hover:bg-surface-strong hover:text-fg"
            >
              Open Automation settings
            </button>
          }
        />

        <div className="flex items-center justify-between gap-3 border-t border-line/60 pt-4">
          <p className="text-[11.5px] text-fg-muted">
            {step === "waiting"
              ? "Waiting for macOS to apply it…"
              : "Nothing else is needed to start."}
          </p>
          <button
            type="button"
            onClick={onFinish}
            className={
              step === "granted" || step === "notWorking"
                ? "shrink-0 rounded-lg border border-accent/30 bg-linear-to-br from-accent to-accent-dim px-3.5 py-1.5 text-[12.5px] font-semibold text-accent-fg"
                : "shrink-0 rounded-lg border border-line bg-surface px-3.5 py-1.5 text-[12.5px] font-medium text-fg hover:bg-surface-strong"
            }
          >
            {step === "granted" || step === "notWorking"
              ? "Continue"
              : "Skip for now"}
          </button>
        </div>
      </section>
    </div>
  );
}

function PermissionRow({
  title,
  detail,
  status,
  action,
}: {
  title: string;
  detail: string | undefined;
  status: React.ReactNode;
  action: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-settings-card-border bg-settings-card p-4">
      <div className="flex items-center justify-between gap-2">
        <span className="text-[13px] font-medium text-fg">{title}</span>
        <span className="shrink-0 rounded-full border border-line bg-surface px-2 py-0.5 font-mono text-[9.5px] uppercase tracking-wider text-fg-muted">
          Optional
        </span>
      </div>
      {detail !== undefined && (
        <p className="text-[12px] leading-relaxed text-fg-muted">{detail}</p>
      )}
      {(status !== null || action !== null) && (
        <div className="flex items-center justify-between gap-2">
          <div className="min-w-0 text-[12px]">{status}</div>
          {action}
        </div>
      )}
    </div>
  );
}

function AccessibilityStatus({
  step,
}: {
  step: Exclude<OnboardingStep, "hidden">;
}) {
  switch (step) {
    case "asking":
      return (
        <span className="text-fg-muted">Off. OpenRize works without it.</span>
      );
    case "waiting":
      return <span className="text-fg-muted">Checking that it works…</span>;
    case "granted":
      return <span className="font-medium text-emerald-500">On.</span>;
    case "notWorking":
      return (
        <span className="text-amber-500">
          Allowed, but not working. Settings says how to fix it.
        </span>
      );
  }
}
