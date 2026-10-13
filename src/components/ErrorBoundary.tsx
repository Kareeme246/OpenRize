import { Component, type ErrorInfo, type ReactNode } from "react";
import * as api from "../lib/api";

interface Props {
  children: ReactNode;
  fallbackTitle?: string;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
  copied: boolean;
  showDetails: boolean;
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
    api
      .recordClientCrash(
        "frontend_error",
        error.message || String(error),
        errorInfo.componentStack || undefined,
        {
          name: error.name,
          stack: error.stack,
        },
      )
      .catch((err: unknown) => {
        console.error("Failed to record client crash:", err);
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

    if (!hasError) {
      return this.props.children;
    }

    return (
      <div className="flex h-screen w-screen flex-col items-center justify-center bg-bg p-6 text-fg">
        <div className="flex max-w-xl flex-col items-center gap-6 rounded-2xl border border-line bg-surface p-8 text-center shadow-2xl">
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

          <div className="flex flex-col gap-2">
            <h1 className="text-[17px] font-semibold text-fg">
              {this.props.fallbackTitle ?? "Something went wrong"}
            </h1>
            <p className="text-[13px] leading-relaxed text-fg-muted">
              OpenRize encountered an unexpected UI error.
            </p>
          </div>

          {error && (
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
          </div>

          {showDetails && (
            <div className="max-h-52 w-full overflow-y-auto rounded-xl border border-line bg-bg p-3 text-left font-mono text-[11px] text-fg-muted">
              <pre className="whitespace-pre-wrap break-all">
                {error?.stack}
                {errorInfo?.componentStack}
              </pre>
            </div>
          )}
        </div>
      </div>
    );
  }
}
