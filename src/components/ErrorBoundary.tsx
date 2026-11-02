import { Component, type ErrorInfo, type ReactNode, useRef } from "react";
import { describeError, reportClientCrash } from "../lib/crashReporting";
import {
  DRAWS_CAPTION_BUTTONS,
  HAS_NATIVE_CAPTION,
  TOP_BAR_PADDING_LEFT,
  TOP_BAR_PADDING_RIGHT,
} from "../lib/platform";
import { CaptionButtons, useWindowMenu } from "./CaptionButtons";

interface Props {
  children: ReactNode;
  fallbackTitle?: string;
  /** For the small panels (menu-bar pulse, break reminder): no bar, no details. */
  compact?: boolean;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
  copied: boolean;
  showDetails: boolean;
}

/**
 * The window's title bar while the app behind it has crashed. The app's own
 * `TopBar` is gone with it, and on macOS (overlaid title bar) and Windows
 * (caption removed) nothing else lets the user move, minimize or close the
 * window, so this keeps the drag area and, where the app draws them, the
 * window buttons.
 */
function FallbackBar() {
  const bar = useRef<HTMLElement>(null);
  useWindowMenu(bar);
  if (HAS_NATIVE_CAPTION) return null;
  return (
    <header
      ref={bar}
      data-tauri-drag-region="deep"
      className={`flex h-11 shrink-0 items-center justify-end border-b border-line bg-bar ${TOP_BAR_PADDING_LEFT} ${TOP_BAR_PADDING_RIGHT}`}
    >
      {DRAWS_CAPTION_BUTTONS && <CaptionButtons />}
    </header>
  );
}

export class ErrorBoundary extends Component<Props, State> {
  public override state: State = {
    hasError: false,
    error: null,
    errorInfo: null,
    copied: false,
    showDetails: false,
  };

  public static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  public override componentDidCatch(error: Error, errorInfo: ErrorInfo): void {
    this.setState({ errorInfo });
    // Rust reduces this to a name, a fixed description and anonymous frames;
    // the message shown below stays on screen and is never recorded.
    reportClientCrash({
      source: "frontend_error",
      ...describeError(error),
      componentStack: errorInfo.componentStack ?? undefined,
    });
  }

  private handleReload = (): void => {
    window.location.reload();
  };

  private handleCopy = (): void => {
    const { error, errorInfo } = this.state;
    const details = [
      `Error: ${error?.name ?? "Error"}: ${error?.message ?? ""}`,
      `Stack: ${error?.stack ?? "none"}`,
      `Component Stack: ${errorInfo?.componentStack ?? "none"}`,
    ].join("\n\n");

    navigator.clipboard
      .writeText(details)
      .then(() => {
        this.setState({ copied: true });
        setTimeout(() => this.setState({ copied: false }), 2500);
      })
      .catch(() => undefined);
  };

  public override render(): ReactNode {
    const { hasError, error, errorInfo, copied, showDetails } = this.state;
    const compact = this.props.compact ?? false;

    if (!hasError) {
      return this.props.children;
    }

    return (
      <div className="flex h-screen w-screen flex-col bg-bg text-fg">
        {!compact && <FallbackBar />}
        <div
          className={`flex min-h-0 flex-1 items-center justify-center overflow-y-auto ${
            compact ? "p-3" : "p-6"
          }`}
        >
          <div
            className={`flex max-w-xl flex-col items-center rounded-2xl border border-line bg-surface text-center shadow-2xl ${
              compact ? "gap-3 p-4" : "gap-6 p-8"
            }`}
          >
            {!compact && (
              <div className="flex size-14 items-center justify-center rounded-2xl border border-danger/30 bg-danger-soft text-danger">
                <svg
                  viewBox="0 0 24 24"
                  className="size-7 fill-none stroke-current stroke-2"
                  aria-hidden="true"
                >
                  <circle cx="12" cy="12" r="10" />
                  <line x1="12" y1="8" x2="12" y2="12" />
                  <line x1="12" y1="16" x2="12.01" y2="16" />
                </svg>
              </div>
            )}

            <div className="flex flex-col gap-2">
              <h1 className="text-[17px] font-semibold text-fg">
                {this.props.fallbackTitle ?? "Something went wrong"}
              </h1>
              <p className="text-[13px] leading-relaxed text-fg-muted">
                OpenRize encountered an unexpected UI error.
              </p>
            </div>

            {!compact && error && (
              <div className="w-full rounded-xl border border-line bg-bg p-3 text-left font-mono text-[12px] text-danger">
                <div className="truncate font-semibold">
                  {error.name}: {error.message}
                </div>
              </div>
            )}

            <div className="flex flex-wrap items-center justify-center gap-3">
              <button
                type="button"
                onClick={this.handleReload}
                className="rounded-xl bg-accent px-4 py-2 text-[13px] font-medium text-accent-fg shadow-sm transition hover:opacity-90"
              >
                Reload app
              </button>
              {!compact && (
                <>
                  <button
                    type="button"
                    onClick={this.handleCopy}
                    className="rounded-xl border border-line bg-surface px-4 py-2 text-[13px] font-medium text-fg hover:bg-surface-strong"
                  >
                    {copied ? "Copied to clipboard!" : "Copy error details"}
                  </button>
                  <button
                    type="button"
                    onClick={() => this.setState({ showDetails: !showDetails })}
                    className="rounded-xl px-3 py-2 text-[12px] text-fg-muted hover:text-fg"
                  >
                    {showDetails ? "Hide details" : "Show details"}
                  </button>
                </>
              )}
            </div>

            {!compact && showDetails && (
              <div className="max-h-52 w-full overflow-y-auto rounded-xl border border-line bg-bg p-3 text-left font-mono text-[11px] text-fg-muted">
                <pre className="whitespace-pre-wrap break-all">
                  {error?.stack}
                  {errorInfo?.componentStack}
                </pre>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }
}
