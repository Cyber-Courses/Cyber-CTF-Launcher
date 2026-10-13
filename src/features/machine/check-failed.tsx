"use client";

import { useState } from "react";
import { RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { EmptyState } from "@/components/ui/empty-state";
import { Spinner } from "@/components/ui/spinner";
import { useT } from "@/lib/i18n";

/**
 * The machine check failed (and none answered before it): what went wrong and Try again, in
 * place of a "Checking this machine…" that would never resolve. The Machine screen, the Overview
 * and the machine-setup window show the same card. The button spins while the check runs again.
 */
export function CheckFailed({ error, onRetry, className }: { error: string; onRetry: () => unknown; className?: string }) {
  const t = useT();
  const [retrying, setRetrying] = useState(false);
  const retry = () => {
    setRetrying(true);
    void Promise.resolve()
      .then(onRetry)
      .catch(() => {
        /* the card stays, with the new error */
      })
      .finally(() => setRetrying(false));
  };
  return (
    <EmptyState
      icon="alert"
      title={t("shell.screen.checkFailed")}
      description={error}
      className={className}
      action={
        <Button variant="outline" size="sm" onClick={retry} disabled={retrying}>
          {retrying ? <Spinner className="size-3.5" /> : <RotateCw className="size-3.5" />} {t("shell.screen.tryAgain")}
        </Button>
      }
    />
  );
}
