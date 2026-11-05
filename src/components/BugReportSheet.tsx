import { openUrl } from "@tauri-apps/plugin-opener";
import {
  type FormEvent,
  type KeyboardEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useToast } from "../hooks/useToast";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import { hasPrimaryModifier, OS_NAME, shortcutLabel } from "../lib/platform";
import type {
  BugDiagnostics,
  BugReportDraft,
  BugReportPayload,
  BugReportResponse,
  DataFolder,
} from "../lib/types";
import { BUTTON_PRIMARY, BUTTON_SECONDARY, TEXTAREA_FIELD } from "./Page";
import { Toggle } from "./SegmentedControl";
import { Field } from "./Sheet";

interface BugReportSheetProps {
  onClose: () => void;
}

const DATA_FOLDER_LABEL: Record<DataFolder, string> = {
  local: "standard data folder",
  legacyRoaming: "older Roaming data folder",
  other: "custom data folder",
};

/** Issue links are only opened when they point at GitHub. */
const GITHUB_ISSUE_PREFIX = "https://github.com/";

/**
 * The report as Markdown, for pasting into a GitHub issue by hand when sending
 * fails. Built from the payload Rust would send, so it holds nothing more.
 */
export function formatReportMarkdown(payload: BugReportPayload): string {
  const lines: string[] = [
    "### What happened?",
    payload.whatHappened,
    "",
    "### What did you expect to happen?",
    payload.whatExpected,
    "",
    "### Steps to reproduce",
    payload.stepsToReproduce,
    "",
  ];
  if (payload.additionalNotes) {
    lines.push("### Additional context / notes", payload.additionalNotes, "");
  }
  if (payload.diagnostics) {
    lines.push(
      "### Diagnostics",
      "",
      "```json",
      JSON.stringify(payload.diagnostics, null, 2),
      "```",
      "",
    );
  }
  return lines.join("\n");
}

export function BugReportSheet({ onClose }: BugReportSheetProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const showToast = useToast();

  const [whatHappened, setWhatHappened] = useState("");
  const [whatExpected, setWhatExpected] = useState("");
  const [steps, setSteps] = useState("");
  const [notes, setNotes] = useState("");
  const [diagnostics, setDiagnostics] = useState<BugDiagnostics | null>(null);
  const [includeEnvironment, setIncludeEnvironment] = useState(true);
  const [includePreferences, setIncludePreferences] = useState(true);
  // Crash summaries can quote what the app was handling, so each is opt-in.
  const [crashIds, setCrashIds] = useState<readonly string[]>([]);
  const [showPreview, setShowPreview] = useState(false);
  const [preview, setPreview] = useState<BugReportPayload | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successResponse, setSuccessResponse] =
    useState<BugReportResponse | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog === null) return;
    dialog.showModal();
    // showModal() focuses the first control, the ✕; start in the form.
    dialog.querySelector<HTMLElement>("form textarea")?.focus();
    return () => dialog.close();
  }, []);

  // What can be attached. Read-only: nothing is sent from here.
  useEffect(() => {
    let active = true;
    api
      .getBugReportDiagnostics(navigator.language)
      .then((data) => {
        if (active) setDiagnostics(data);
      })
      .catch((cause: unknown) => {
        console.error("Failed to load bug diagnostics:", describeError(cause));
      });
    return () => {
      active = false;
    };
  }, []);

  const requiredFilled =
    whatHappened.trim() !== "" &&
    whatExpected.trim() !== "" &&
    steps.trim() !== "";
  const canSubmit = requiredFilled && !submitting;

  const draft = useMemo<BugReportDraft>(
    () => ({
      whatHappened,
      whatExpected,
      stepsToReproduce: steps,
      additionalNotes: notes.trim() || undefined,
      attachments: {
        environment: includeEnvironment,
        preferences: includePreferences,
        crashIds: [...crashIds],
      },
      locale: navigator.language,
    }),
    [
      whatHappened,
      whatExpected,
      steps,
      notes,
      includeEnvironment,
      includePreferences,
      crashIds,
    ],
  );

  // The preview is built by Rust from the same draft Send uses, so it is
  // exactly what would go out.
  useEffect(() => {
    if (!showPreview || !requiredFilled) {
      setPreview(null);
      setPreviewError(null);
      return;
    }
    let active = true;
    const timer = window.setTimeout(() => {
      api
        .previewBugReport(draft)
        .then((payload) => {
          if (!active) return;
          setPreview(payload);
          setPreviewError(null);
        })
        .catch((cause: unknown) => {
          if (!active) return;
          setPreview(null);
          setPreviewError(describeError(cause));
        });
    }, 200);
    return () => {
      active = false;
      window.clearTimeout(timer);
    };
  }, [showPreview, requiredFilled, draft]);

  const handleSubmit = async (event: FormEvent): Promise<void> => {
    event.preventDefault();
    if (!canSubmit) return;

    setSubmitting(true);
    setError(null);
    try {
      const response = await api.submitBugReport(draft);
      setSuccessResponse(response);
      showToast({ message: "Bug report sent", tone: "success" });
    } catch (cause) {
      setError(describeError(cause));
    } finally {
      setSubmitting(false);
    }
  };

  const handleCopyMarkdown = (): void => {
    api
      .previewBugReport(draft)
      .then((payload) =>
        navigator.clipboard.writeText(formatReportMarkdown(payload)),
      )
      .then(() => {
        setCopied(true);
        showToast({ message: "Report copied to clipboard", tone: "neutral" });
        setTimeout(() => setCopied(false), 2500);
      })
      .catch((cause: unknown) => {
        setError(describeError(cause));
      });
  };

  const toggleCrash = (id: string, on: boolean): void => {
    setCrashIds((current) =>
      on ? [...current, id] : current.filter((existing) => existing !== id),
    );
  };

  // The platform's primary modifier with Enter sends, as the other sheets do.
  const handleKeyDown = (event: KeyboardEvent<HTMLFormElement>): void => {
    if (event.key === "Enter" && hasPrimaryModifier(event) && canSubmit) {
      event.preventDefault();
      void handleSubmit(event);
    }
  };

  const issueUrl = successResponse?.issueUrl ?? "";
  const canOpenIssue = issueUrl.startsWith(GITHUB_ISSUE_PREFIX);

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop click-to-cancel; native onCancel (Escape) already provides the keyboard equivalent
    <dialog
      ref={dialogRef}
      onCancel={(e) => {
        e.preventDefault();
        if (!submitting) onClose();
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget && !submitting) onClose();
      }}
      className="m-auto rounded-2xl bg-transparent p-0 text-fg outline-none backdrop:bg-scrim"
    >
      <div className="flex max-h-[calc(100vh-3rem)] w-[620px] max-w-[calc(100vw-2rem)] flex-col rounded-2xl border border-line bg-panel shadow-2xl">
        <div className="flex items-center justify-between border-line border-b px-5 py-3">
          <h2 className="font-semibold text-[14px] text-fg-strong">
            Report a bug
          </h2>
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            aria-label="Close"
            className="rounded p-1 text-fg-faint hover:bg-surface hover:text-fg disabled:opacity-40"
          >
            ✕
          </button>
        </div>

        {successResponse ? (
          <div className="flex flex-col items-center gap-4 px-6 py-8 text-center">
            <div className="flex size-12 items-center justify-center rounded-full bg-success-soft text-success">
              <svg
                viewBox="0 0 24 24"
                className="size-6 fill-none stroke-2 stroke-current"
                aria-hidden="true"
              >
                <polyline points="20 6 9 17 4 12" />
              </svg>
            </div>
            <div className="space-y-1">
              <h3 className="font-semibold text-[15px] text-fg-strong">
                Issue #{successResponse.issueNumber} created
              </h3>
              <p className="max-w-md text-[12.5px] text-fg-muted">
                Thank you. Your report is on GitHub, where you can follow the
                replies.
              </p>
            </div>
            <div className="mt-2 flex items-center gap-3">
              {canOpenIssue && (
                <button
                  type="button"
                  onClick={() => {
                    openUrl(issueUrl).catch((cause: unknown) =>
                      showToast({
                        tone: "error",
                        message: describeError(cause),
                      }),
                    );
                  }}
                  className={BUTTON_PRIMARY}
                >
                  View on GitHub
                </button>
              )}
              <button
                type="button"
                onClick={onClose}
                className={BUTTON_SECONDARY}
              >
                Done
              </button>
            </div>
          </div>
        ) : (
          <form
            onSubmit={handleSubmit}
            onKeyDown={handleKeyDown}
            className="flex min-h-0 flex-1 flex-col"
          >
            <div className="min-h-0 flex-1 space-y-3.5 overflow-y-auto px-5 py-4 text-[12px]">
              <div className="rounded-lg border border-line bg-inset-soft px-3 py-2 text-[11.5px] text-fg-muted leading-relaxed">
                <span className="font-medium text-fg-soft">
                  Reports become public GitHub issues.
                </span>{" "}
                Anything you type is posted as written, so leave out client
                names, project names and other private details. Nothing is sent
                until you press Send.
              </div>

              <Field
                label="What happened?"
                htmlFor="bug-what-happened"
                hint="Describe the problem or unexpected behavior you ran into"
              >
                <textarea
                  id="bug-what-happened"
                  required
                  rows={2}
                  value={whatHappened}
                  onChange={(e) => setWhatHappened(e.target.value)}
                  placeholder="e.g. The timer stopped updating after my computer woke from sleep"
                  className={TEXTAREA_FIELD}
                />
              </Field>

              <Field
                label="What did you expect to happen?"
                htmlFor="bug-what-expected"
                hint="Describe how OpenRize should have behaved"
              >
                <textarea
                  id="bug-what-expected"
                  required
                  rows={2}
                  value={whatExpected}
                  onChange={(e) => setWhatExpected(e.target.value)}
                  placeholder="e.g. The timer should keep counting and keep the time so far"
                  className={TEXTAREA_FIELD}
                />
              </Field>

              <Field
                label="Steps to reproduce"
                htmlFor="bug-steps"
                hint="What sequence of actions leads to the problem?"
              >
                <textarea
                  id="bug-steps"
                  required
                  rows={3}
                  value={steps}
                  onChange={(e) => setSteps(e.target.value)}
                  placeholder={
                    "1. Start a stopwatch\n2. Lock the screen for 5 minutes\n3. Unlock and look at the timer"
                  }
                  className={TEXTAREA_FIELD}
                />
              </Field>

              <Field
                label="Additional context (optional)"
                htmlFor="bug-notes"
                hint="Anything else that helps, such as an error message you saw"
              >
                <textarea
                  id="bug-notes"
                  rows={2}
                  value={notes}
                  onChange={(e) => setNotes(e.target.value)}
                  placeholder={`e.g. It started right after I updated ${OS_NAME}`}
                  className={TEXTAREA_FIELD}
                />
              </Field>

              <fieldset className="space-y-3 rounded-xl border border-line bg-surface p-3.5">
                <legend className="px-1 font-semibold text-[12px] text-fg">
                  Attachments
                </legend>

                <AttachmentRow
                  title="App and system details"
                  description={
                    diagnostics
                      ? `OpenRize v${diagnostics.environment.appVersion}, ${diagnostics.environment.os} ${diagnostics.environment.osVersion} (${diagnostics.environment.arch}), language ${diagnostics.environment.locale}, ${DATA_FOLDER_LABEL[diagnostics.environment.dataFolder]}`
                      : "Loading…"
                  }
                  checked={includeEnvironment}
                  onChange={setIncludeEnvironment}
                  disabled={diagnostics === null}
                />
                <AttachmentRow
                  title="Preferences"
                  description="Theme, accent, window and notification options, retention, work hours and AI toggles. Your custom AI prompt, projects and clients are never included."
                  checked={includePreferences}
                  onChange={setIncludePreferences}
                  disabled={diagnostics === null}
                />

                {diagnostics && diagnostics.recentCrashes.length > 0 && (
                  <div className="space-y-1.5 border-line border-t pt-3">
                    <div className="font-medium text-[12px] text-fg">
                      Recent crashes on this computer
                    </div>
                    <div className="text-[11px] text-fg-muted">
                      Error text can mention what the app was working on. Read
                      each one and tick only what you are happy to share.
                    </div>
                    {diagnostics.recentCrashes.map((crash) => {
                      const id = `bug-crash-${crash.id}`;
                      return (
                        <label
                          key={crash.id}
                          htmlFor={id}
                          className="flex cursor-pointer items-start gap-2 rounded-md border border-line-soft bg-inset-soft px-2.5 py-2"
                        >
                          <input
                            id={id}
                            type="checkbox"
                            checked={crashIds.includes(crash.id)}
                            onChange={(e) =>
                              toggleCrash(crash.id, e.target.checked)
                            }
                            className="mt-0.5 accent-accent"
                          />
                          <span className="min-w-0 text-[11px]">
                            <span className="block text-fg-muted">
                              {new Date(crash.timestampMs).toLocaleString()} ·{" "}
                              {crash.source} · v{crash.appVersion}
                            </span>
                            <span className="block break-words font-mono text-fg-soft">
                              {crash.message || "(no message)"}
                            </span>
                          </span>
                        </label>
                      );
                    })}
                  </div>
                )}

                <div className="border-line border-t pt-3">
                  <div className="flex items-center justify-between gap-3 text-[11px]">
                    <span className="text-fg-muted">
                      A report never includes window titles, web addresses, time
                      entries, projects, clients or your database.
                    </span>
                    <button
                      type="button"
                      onClick={() => setShowPreview(!showPreview)}
                      aria-expanded={showPreview}
                      className="shrink-0 text-accent hover:underline"
                    >
                      {showPreview
                        ? "Hide what will be sent"
                        : "Show what will be sent"}
                    </button>
                  </div>
                  {showPreview && (
                    <div className="mt-2.5 max-h-52 overflow-y-auto rounded-lg border border-line bg-inset p-2.5 font-mono text-[10.5px] text-fg-muted">
                      {!requiredFilled && (
                        <p>Fill in the first three boxes to see the report.</p>
                      )}
                      {previewError && (
                        <p className="text-danger">{previewError}</p>
                      )}
                      {preview && (
                        <pre className="whitespace-pre-wrap break-all">
                          {JSON.stringify(preview, null, 2)}
                        </pre>
                      )}
                    </div>
                  )}
                </div>
              </fieldset>
            </div>

            {error && (
              <div
                role="alert"
                className="mx-5 mb-2 space-y-1.5 rounded-md border border-danger/30 bg-danger-soft px-3 py-2 text-[11.5px] text-danger"
              >
                <div>{error}</div>
                <div className="text-fg-muted">
                  Your text is still here. Try again later, or copy the report
                  and paste it into a new issue on GitHub yourself.
                </div>
              </div>
            )}

            <div className="flex items-center justify-between border-line border-t px-5 py-3">
              <button
                type="button"
                onClick={handleCopyMarkdown}
                disabled={!requiredFilled}
                title="Copy the report as Markdown, to paste into a GitHub issue"
                className="text-[11.5px] text-fg-muted hover:text-fg disabled:opacity-40"
              >
                {copied ? "Copied" : "Copy report"}
              </button>

              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={onClose}
                  disabled={submitting}
                  className={BUTTON_SECONDARY}
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={!canSubmit}
                  title={`Send (${shortcutLabel("Enter")})`}
                  className={BUTTON_PRIMARY}
                >
                  {submitting ? "Sending…" : "Send report"}
                </button>
              </div>
            </div>
          </form>
        )}
      </div>
    </dialog>
  );
}

function AttachmentRow({
  title,
  description,
  checked,
  onChange,
  disabled,
}: {
  title: string;
  description: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div className="min-w-0">
        <div className="font-medium text-[12px] text-fg">{title}</div>
        <div className="text-[11px] text-fg-muted">{description}</div>
      </div>
      <Toggle
        label={title}
        checked={checked}
        onChange={onChange}
        disabled={disabled}
      />
    </div>
  );
}
