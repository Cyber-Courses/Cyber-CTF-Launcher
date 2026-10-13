"use client";

import { Component, useEffect, useState, type ReactNode } from "react";
import { AlertTriangle, Copy, RefreshCw, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { tell } from "@/lib/failure";
import { logFrontendError } from "@/lib/error-log";
import { useT } from "@/lib/i18n";

/** The error as text to paste into a bug report: message, window, agent and stack. Kept in
 *  English whatever the language: it is read by us, not shown in the app. */
function errorDetails(error: Error & { digest?: string }): string {
  return [
    `Error: ${error.message || String(error)}`,
    error.digest ? `Digest: ${error.digest}` : null,
    `Window: ${typeof window === "undefined" ? "" : window.location.pathname + window.location.search}`,
    `Agent: ${typeof navigator === "undefined" ? "" : navigator.userAgent}`,
    error.stack ? `\n${error.stack}` : null,
  ]
    .filter(Boolean)
    .join("\n");
}

/**
 * A crash, in the app's own style (a lit panel, the message in a log well):
 * what went wrong, a retry, and the details to copy. Used inline in a step and as a full window.
 */
function ErrorPanel({
  error,
  retry,
  title: titleProp,
  description: descriptionProp,
  className,
}: {
  error: Error & { digest?: string };
  retry?: () => void;
  title?: string;
  description?: string;
  className?: string;
}) {
  const t = useT();
  const title = titleProp ?? t("shell.error.title");
  const description = descriptionProp ?? t("shell.error.description");
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    console.error(error);
    logFrontendError("error screen", errorDetails(error));
  }, [error]);
  const copy = () =>
    navigator.clipboard
      .writeText(errorDetails(error))
      .then(() => setCopied(true))
      .catch(tell(t("errors.clipboard")));

  return (
    <div className={cn("surface-panel flex flex-col items-center rounded-panel px-6 py-9 text-center", className)}>
      <div className="flex size-11 items-center justify-center rounded-control bg-destructive/10 text-destructive shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--destructive)_30%,transparent)]">
        <AlertTriangle className="size-5" />
      </div>
      <h2 className="serif-title mt-4 text-[1.5rem] text-foreground">{title}</h2>
      <p className="mt-2 max-w-sm text-[0.8125rem] leading-relaxed text-muted-foreground">{description}</p>
      <p
        role="alert"
        className="surface-log mt-5 max-h-32 w-full max-w-md overflow-auto rounded-control p-3 text-left font-mono text-[0.71875rem] break-words whitespace-pre-wrap text-destructive"
      >
        {error.message || String(error) || t("shell.error.unknown")}
      </p>
      <div className="mt-5 flex flex-wrap justify-center gap-2">
        {retry && (
          <Button size="sm" onClick={retry}>
            <RotateCcw className="size-3.5" /> {t("shell.error.tryAgain")}
          </Button>
        )}
        <Button variant="outline" size="sm" onClick={() => window.location.reload()}>
          <RefreshCw className="size-3.5" /> {t("shell.error.reload")}
        </Button>
        <Button variant="outline" size="sm" onClick={copy}>
          <Copy className="size-3.5" /> {copied ? t("shell.error.copied") : t("shell.error.copyDetails")}
        </Button>
      </div>
    </div>
  );
}

/** A whole window that crashed (app/error.tsx, app/global-error.tsx). */
export function ErrorScreen({ error, retry }: { error: Error & { digest?: string }; retry?: () => void }) {
  return (
    <div className="flex min-h-dvh items-center justify-center bg-background px-6 py-10 text-foreground">
      <ErrorPanel error={error} retry={retry} className="w-full max-w-lg" />
    </div>
  );
}

/**
 * Keeps a crash inside one part of a window: the frame around it (header, progress, Back) stays
 * usable. `resetKey` clears the error when it changes, e.g. on moving to another step.
 */
export class ErrorBoundary extends Component<{ children: ReactNode; resetKey?: unknown; title?: string }, { error: Error | null; key?: unknown }> {
  state: { error: Error | null; key?: unknown } = { error: null, key: this.props.resetKey };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  static getDerivedStateFromProps(props: { resetKey?: unknown }, state: { error: Error | null; key?: unknown }) {
    return props.resetKey !== state.key ? { error: null, key: props.resetKey } : null;
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return <StepErrorPanel error={error} title={this.props.title} retry={() => this.setState({ error: null })} />;
  }
}

/** The boundary's panel: a function component, so it can read the current language. */
function StepErrorPanel({ error, title, retry }: { error: Error; title?: string; retry: () => void }) {
  const t = useT();
  return <ErrorPanel error={error} title={title} description={t("shell.error.stepDescription")} retry={retry} />;
}
