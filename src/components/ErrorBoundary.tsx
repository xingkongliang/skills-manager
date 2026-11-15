import { Component, type ErrorInfo, type ReactNode } from "react";
// The bare i18next instance, not ../i18n: the fallback must not depend on the
// app's i18n bootstrap having succeeded. If it never initialized, tr() below
// falls back to the hard-coded English strings.
import i18n from "i18next";
import { writeText as clipboardWriteText } from "@tauri-apps/plugin-clipboard-manager";
import { exportLogsZip, logStartupEvent } from "../lib/tauri";

function tr(key: string, fallback: string, vars: Record<string, string> = {}): string {
  try {
    if (i18n.isInitialized) {
      const value = i18n.t(key, { defaultValue: fallback, ...vars });
      if (typeof value === "string" && value && value !== key) return value;
    }
  } catch {
    // i18n itself may be what broke; use the English fallback.
  }
  return Object.entries(vars).reduce(
    (text, [name, value]) => text.split(`{{${name}}}`).join(value),
    fallback,
  );
}

function describeError(error: unknown, componentStack?: string | null): string {
  const head = error instanceof Error
    ? error.stack || `${error.name}: ${error.message}`
    : String(error);
  return componentStack ? `${head}\n\nComponent stack:${componentStack}` : head;
}

type ExportState =
  | { kind: "idle" }
  | { kind: "busy" }
  | { kind: "done"; path: string }
  | { kind: "failed" };

interface State {
  error: unknown;
  hasError: boolean;
  componentStack: string | null;
  copied: boolean;
  exportState: ExportState;
}

/**
 * Top-level render-error boundary. Without it, any exception thrown while
 * rendering unmounts the whole React root and leaves a blank window (#613).
 * Only catches render/lifecycle errors; event-handler and async errors are
 * handled where they happen.
 */
export class ErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = {
    error: null,
    hasError: false,
    componentStack: null,
    copied: false,
    exportState: { kind: "idle" },
  };

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error, hasError: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo) {
    this.setState({ componentStack: info.componentStack ?? null });
    console.error("Render error caught by top-level ErrorBoundary", error, info.componentStack);
    // The only frontend-to-backend log command is log_startup_event; the
    // backend keeps the first 64 characters of the label.
    const summary = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
    try {
      logStartupEvent(`render_error ${summary}`, performance.now()).catch(() => {});
    } catch {
      // No Tauri bridge (plain browser); the console line above is all we get.
    }
  }

  private handleCopy = async () => {
    const text = describeError(this.state.error, this.state.componentStack);
    try {
      await clipboardWriteText(text);
    } catch {
      try {
        await navigator.clipboard.writeText(text);
      } catch (err) {
        console.error("Failed to copy error details", err);
        return;
      }
    }
    this.setState({ copied: true });
  };

  private handleExportLogs = async () => {
    this.setState({ exportState: { kind: "busy" } });
    try {
      const result = await exportLogsZip();
      this.setState({ exportState: { kind: "done", path: result.zip_path } });
    } catch (err) {
      console.error("Failed to export logs", err);
      this.setState({ exportState: { kind: "failed" } });
    }
  };

  render() {
    if (!this.state.hasError) return this.props.children;

    const { copied, exportState } = this.state;
    const details = describeError(this.state.error, this.state.componentStack);

    return (
      <div
        role="alert"
        className="fixed inset-0 z-50 flex items-center justify-center overflow-auto bg-background p-6"
      >
        <div className="app-panel w-full max-w-[640px] p-5 shadow-card">
          <h1 className="text-[14px] font-semibold text-primary">
            {tr("errorBoundary.title", "Something went wrong")}
          </h1>
          <p className="mt-1 text-[13px] text-muted">
            {tr(
              "errorBoundary.description",
              "The window hit an unexpected error. Reload to continue. If it keeps happening, copy the details below into a GitHub issue.",
            )}
          </p>
          <pre className="mt-4 max-h-64 select-text overflow-auto whitespace-pre-wrap break-words rounded-lg border border-border-subtle bg-bg-secondary p-3 font-mono text-[12px] text-secondary">
            {details}
          </pre>
          {exportState.kind === "done" && (
            <p className="mt-3 select-text break-all text-[12px] text-muted">
              {tr("errorBoundary.exportLogsDone", "Logs saved to {{path}}", { path: exportState.path })}
            </p>
          )}
          {exportState.kind === "failed" && (
            <p className="mt-3 text-[12px] text-danger">
              {tr("errorBoundary.exportLogsFailed", "Failed to export logs.")}
            </p>
          )}
          <div className="mt-4 flex flex-wrap justify-end gap-2">
            <button type="button" className="app-button-secondary" onClick={() => void this.handleCopy()}>
              {copied
                ? tr("errorBoundary.copied", "Copied")
                : tr("errorBoundary.copy", "Copy details")}
            </button>
            <button
              type="button"
              className="app-button-secondary"
              disabled={exportState.kind === "busy"}
              onClick={() => void this.handleExportLogs()}
            >
              {tr("errorBoundary.exportLogs", "Export logs")}
            </button>
            <button type="button" className="app-button-primary" onClick={() => window.location.reload()}>
              {tr("errorBoundary.reload", "Reload")}
            </button>
          </div>
        </div>
      </div>
    );
  }
}
