import { openUrl } from "@tauri-apps/plugin-opener";
import { type FormEvent, useEffect, useRef, useState } from "react";
import { useSettings } from "../hooks/useSettings";
import { useToast } from "../hooks/useToast";
import * as api from "../lib/api";
import { describeError } from "../lib/api";
import type {
  BugDiagnostics,
  BugReportPayload,
  BugReportResponse,
} from "../lib/types";
import { BUTTON_PRIMARY, BUTTON_SECONDARY, TEXTAREA_FIELD } from "./Page";
import { Toggle } from "./SegmentedControl";
import { Field } from "./Sheet";

interface BugReportSheetProps {
  onClose: () => void;
}

/** Formats a bug report payload into Markdown for copying or file export. */
export function formatReportMarkdown(
  whatHappened: string,
  whatExpected: string,
  steps: string,
  notes: string,
  diagnostics: BugDiagnostics | null,
  installId: string,
): string {
  const lines: string[] = [
    "# OpenRize Bug Report",
    "",
    "### What happened?",
    whatHappened.trim() || "No description provided.",
    "",
    "### What did you expect to happen?",
    whatExpected.trim() || "No expected behavior described.",
    "",
    "### Steps to reproduce",
    steps.trim() || "No steps provided.",
    "",
  ];

  if (notes.trim()) {
    lines.push("### Additional context / notes", notes.trim(), "");
  }

  lines.push("---", "");

  if (diagnostics) {
    lines.push(
      "### Diagnostics",
      `- **App Version:** \`v${diagnostics.appVersion}\``,
      `- **OS:** \`${diagnostics.os} ${diagnostics.osVersion} (${diagnostics.arch})\``,
      `- **Locale:** \`${diagnostics.locale}\``,
      `- **Install ID:** \`${installId ? `${installId.slice(0, 8)}...` : "none"}\``,
      `- **App Data Dir:** \`${diagnostics.appDataDir || "standard"}\``,
      "",
      "<details>",
      "<summary>App Settings</summary>",
      "",
      "```json",
      JSON.stringify(diagnostics.settings, null, 2),
      "```",
      "</details>",
      "",
    );

    if (diagnostics.recentCrashes && diagnostics.recentCrashes.length > 0) {
      lines.push(
        "<details>",
        `<summary>Recent Local Crashes (${diagnostics.recentCrashes.length})</summary>`,
        "",
        "```json",
        JSON.stringify(diagnostics.recentCrashes, null, 2),
        "```",
        "</details>",
        "",
      );
    }
  } else {
    lines.push("### Diagnostics", "*Diagnostics omitted by user.*", "");
  }

  lines.push("> *Reported via OpenRize in-app bug report.*");
  return lines.join("\n");
}

export function BugReportSheet({ onClose }: BugReportSheetProps) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const { settings } = useSettings();
  const showToast = useToast();

  const [whatHappened, setWhatHappened] = useState("");
  const [whatExpected, setWhatExpected] = useState("");
  const [steps, setSteps] = useState("");
  const [notes, setNotes] = useState("");
  const [includeDiagnostics, setIncludeDiagnostics] = useState(true);
  const [diagnostics, setDiagnostics] = useState<BugDiagnostics | null>(null);
  const [showPreview, setShowPreview] = useState(false);

  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successResponse, setSuccessResponse] =
    useState<BugReportResponse | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    const dialog = dialogRef.current;
    if (dialog === null) return;
    dialog.showModal();
    return () => dialog.close();
  }, []);

  // Fetch diagnostics on mount
  useEffect(() => {
    let active = true;
    api
      .getBugReportDiagnostics(navigator.language)
      .then((data) => {
        if (active) setDiagnostics(data);
      })
      .catch((err) => {
        console.error("Failed to load bug diagnostics:", err);
      });
    return () => {
      active = false;
    };
  }, []);

  const canSubmit =
    whatHappened.trim() !== "" &&
    whatExpected.trim() !== "" &&
    steps.trim() !== "" &&
    !submitting;

  const handleSubmit = async (e: FormEvent): Promise<void> => {
    e.preventDefault();
    if (!canSubmit) return;

    setSubmitting(true);
    setError(null);

    const payload: BugReportPayload = {
      installId: settings.installId || "anonymous-install",
      whatHappened: whatHappened.trim(),
      whatExpected: whatExpected.trim(),
      stepsToReproduce: steps.trim(),
      additionalNotes: notes.trim() || undefined,
      diagnostics: includeDiagnostics ? diagnostics : null,
    };

    try {
      const response = await api.submitBugReport(payload);
      setSuccessResponse(response);
      showToast({ message: "Bug report submitted!", tone: "success" });
    } catch (cause) {
      console.error("Bug report submission failed:", describeError(cause));
      setError("Unable to submit report. Please try again in a few minutes.");
    } finally {
      setSubmitting(false);
    }
  };

  const handleCopyMarkdown = (): void => {
    const md = formatReportMarkdown(
      whatHappened,
      whatExpected,
      steps,
      notes,
      includeDiagnostics ? diagnostics : null,
      settings.installId,
    );
    navigator.clipboard
      .writeText(md)
      .then(() => {
        setCopied(true);
        showToast({ message: "Report copied to clipboard", tone: "neutral" });
        setTimeout(() => setCopied(false), 2500);
      })
      .catch(() => {
        setError("Failed to copy report to clipboard.");
      });
  };

  const handleSaveFile = (): void => {
    const md = formatReportMarkdown(
      whatHappened,
      whatExpected,
      steps,
      notes,
      includeDiagnostics ? diagnostics : null,
      settings.installId,
    );
    const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `openrize-bug-report-${Date.now()}.md`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    showToast({ message: "Saved bug report file", tone: "success" });
  };

  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: backdrop click-to-cancel
    <dialog
      ref={dialogRef}
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      className="m-auto rounded-2xl bg-transparent p-0 text-fg outline-none backdrop:bg-scrim"
    >
      <div className="flex max-h-[calc(100vh-3rem)] w-[620px] max-w-[calc(100vw-2rem)] flex-col rounded-2xl border border-line bg-panel shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between border-line border-b px-5 py-3.5">
          <div className="flex items-center gap-2">
            <svg
              viewBox="0 0 24 24"
              className="size-4 shrink-0 stroke-current text-fg-soft stroke-2 fill-none"
              aria-hidden="true"
            >
              <path d="M12 2a4 4 0 0 0-4 4v1H6a2 2 0 0 0-2 2v2a2 2 0 0 0 2 2h1v3a5 5 0 0 0 10 0v-3h1a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-2V6a4 4 0 0 0-4-4z" />
              <line x1="8" y1="12" x2="16" y2="12" />
              <line x1="12" y1="9" x2="12" y2="15" />
            </svg>
            <h2 className="font-semibold text-[14.5px] text-fg-strong">
              Report a bug
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="rounded p-1 text-fg-faint transition-colors hover:bg-surface hover:text-fg cursor-pointer"
          >
            ✕
          </button>
        </div>

        {/* Content */}
        {successResponse ? (
          <div className="flex flex-col items-center gap-4 px-6 py-8 text-center">
            <div className="flex size-12 items-center justify-center rounded-full bg-success/15 text-success">
              <svg
                viewBox="0 0 24 24"
                className="size-6 stroke-current stroke-2 fill-none"
                aria-hidden="true"
              >
                <polyline points="20 6 9 17 4 12" />
              </svg>
            </div>
            <div className="space-y-1">
              <h3 className="font-semibold text-[15px] text-fg-strong">
                Issue #{successResponse.issueNumber} created
              </h3>
              <p className="text-[12.5px] text-fg-muted max-w-md">
                Thank you! Your bug report has been filed on GitHub with the
                team. You can track updates directly on the issue page.
              </p>
            </div>
            <div className="mt-2 flex items-center gap-3">
              <button
                type="button"
                onClick={() => openUrl(successResponse.issueUrl)}
                className={BUTTON_PRIMARY}
              >
                View on GitHub →
              </button>
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
            className="flex min-h-0 flex-1 flex-col"
          >
            <div className="min-h-0 flex-1 space-y-4 overflow-y-auto px-5 py-4 text-[12px]">
              {/* Error & Fallback Banner */}
              {error && (
                <div
                  role="alert"
                  className="rounded-lg border border-danger/30 bg-danger-soft p-3 text-[12px] text-danger space-y-2.5"
                >
                  <div className="font-medium">{error}</div>
                  <div className="text-[11.5px] text-fg-muted">
                    Your words are preserved below. You can copy the report to
                    paste into GitHub, save it as a markdown file, or try again
                    in a few minutes:
                  </div>
                  <div className="flex flex-wrap items-center gap-2 pt-0.5">
                    <button
                      type="button"
                      onClick={handleCopyMarkdown}
                      className="rounded border border-line bg-surface px-2.5 py-1 text-[11px] font-medium text-fg hover:bg-surface-strong transition-colors cursor-pointer"
                    >
                      {copied ? "Copied!" : "Copy report to clipboard"}
                    </button>
                    <button
                      type="button"
                      onClick={handleSaveFile}
                      className="rounded border border-line bg-surface px-2.5 py-1 text-[11px] font-medium text-fg hover:bg-surface-strong transition-colors cursor-pointer"
                    >
                      Save report as file
                    </button>
                  </div>
                </div>
              )}

              {/* Form fields */}
              <Field
                label="What happened?"
                htmlFor="bug-what-happened"
                hint="Describe the problem or unexpected behavior you encountered"
              >
                <textarea
                  id="bug-what-happened"
                  required
                  rows={2}
                  value={whatHappened}
                  onChange={(e) => setWhatHappened(e.target.value)}
                  placeholder="e.g. Timer stopped updating after my laptop woke from sleep"
                  className={TEXTAREA_FIELD}
                />
              </Field>

              <Field
                label="What did you expect to happen?"
                htmlFor="bug-what-expected"
                hint="Describe how OpenRize was supposed to behave"
              >
                <textarea
                  id="bug-what-expected"
                  required
                  rows={2}
                  value={whatExpected}
                  onChange={(e) => setWhatExpected(e.target.value)}
                  placeholder="e.g. The timer should continue counting and preserve accumulated time"
                  className={TEXTAREA_FIELD}
                />
              </Field>

              <Field
                label="Steps to reproduce"
                htmlFor="bug-steps"
                hint="What sequence of actions leads to the glitch?"
              >
                <textarea
                  id="bug-steps"
                  required
                  rows={3}
                  value={steps}
                  onChange={(e) => setSteps(e.target.value)}
                  placeholder="1. Start a stopwatch&#10;2. Lock the screen for 5 minutes&#10;3. Unlock and observe timer"
                  className={TEXTAREA_FIELD}
                />
              </Field>

              <Field
                label="Additional context / notes (optional)"
                htmlFor="bug-notes"
                hint="Any extra details, hardware info, or error messages you saw"
              >
                <textarea
                  id="bug-notes"
                  rows={2}
                  value={notes}
                  onChange={(e) => setNotes(e.target.value)}
                  placeholder="e.g. Happened right after updating to macOS 15.1"
                  className={TEXTAREA_FIELD}
                />
              </Field>

              {/* Diagnostics Attachment Section */}
              <div className="rounded-xl border border-line bg-surface/50 p-3.5 space-y-3">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="font-semibold text-[12px] text-fg">
                      Include diagnostic report
                    </div>
                    <div className="text-[11px] text-fg-muted">
                      Attaches app version, OS info, non-sensitive preferences,
                      and recent crash log summaries.
                    </div>
                  </div>
                  <Toggle
                    label="Include diagnostic report"
                    checked={includeDiagnostics}
                    onChange={(checked) => setIncludeDiagnostics(checked)}
                  />
                </div>

                {includeDiagnostics && diagnostics && (
                  <div className="border-t border-line/50 pt-2.5">
                    <div className="flex items-center justify-between text-[11px]">
                      <span className="text-fg-soft font-mono">
                        v{diagnostics.appVersion} · {diagnostics.os}{" "}
                        {diagnostics.osVersion} ({diagnostics.arch})
                      </span>
                      <button
                        type="button"
                        onClick={() => setShowPreview(!showPreview)}
                        className="text-accent hover:underline cursor-pointer"
                      >
                        {showPreview ? "Hide preview" : "View payload preview"}
                      </button>
                    </div>

                    {showPreview && (
                      <div className="mt-2.5 max-h-44 overflow-y-auto rounded-lg border border-line bg-inset p-2.5 font-mono text-[10.5px] text-fg-muted">
                        <pre className="whitespace-pre-wrap break-all">
                          {JSON.stringify(diagnostics, null, 2)}
                        </pre>
                      </div>
                    )}
                  </div>
                )}

                {/* Privacy Guarantee Notice */}
                <div className="rounded-lg bg-surface-2 p-2.5 text-[11px] text-fg-muted leading-relaxed">
                  <div className="font-medium text-fg-soft mb-0.5">
                    What is NOT sent:
                  </div>
                  Window titles, browser URLs, client details, project names,
                  time-entry descriptions, and personal filesystem paths are
                  never captured or transmitted.
                </div>
              </div>
            </div>

            {/* Footer */}
            <div className="flex items-center justify-between border-line border-t px-5 py-3">
              <button
                type="button"
                onClick={handleCopyMarkdown}
                title="Copy formatted markdown to clipboard"
                className="text-[11.5px] text-fg-muted hover:text-fg transition-colors cursor-pointer"
              >
                {copied ? "Copied markdown!" : "Copy markdown"}
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
                  className={BUTTON_PRIMARY}
                >
                  {submitting ? "Submitting…" : "Submit report"}
                </button>
              </div>
            </div>
          </form>
        )}
      </div>
    </dialog>
  );
}
