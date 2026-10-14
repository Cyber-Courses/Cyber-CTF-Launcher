"use client";

import { X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { CalloutRow } from "@/features/machine/machine-parts";
import { failureHintKey, failureTitleKey, type FailureKind } from "@/features/labs/failure-hints";
import type { LabOperation } from "@/features/labs/lab-detail/lab-state";
import { useT } from "@/lib/i18n";
import { describeError, type CommandError } from "@/lib/errors";

/**
 * The last operation on the lab failed: said right under the header, beside its actions (at the
 * top of the page, in view), with the error, a hint when the cause is a known one, and, after a
 * failed removal, that part of the lab is still up.
 */
export function FailureNotice({
  title,
  op,
  message,
  kind,
  stillUp,
  onDismiss,
}: {
  /** The lab's title. */
  title: string;
  op: LabOperation;
  message: string;
  /** What kind of failure it is, when it has a known fix. */
  kind: FailureKind | null;
  /** The lab still runs (a stop that failed). */
  stillUp: boolean;
  onDismiss: () => void;
}) {
  const t = useT();
  const key = failureTitleKey(op);
  if (!key) return null;
  return (
    <CalloutRow
      tone="fail"
      title={t(key, { title })}
      actions={
        <Button variant="ghost" size="xs" onClick={onDismiss} aria-label={t("labs.failure.dismiss")} title={t("labs.failure.dismiss")}>
          <X />
        </Button>
      }
    >
      <span className="block break-words font-mono text-[0.6875rem]">{message}</span>
      {kind && <span className="mt-1 block text-foreground">{t(failureHintKey(kind))}</span>}
      {stillUp && <span className="mt-1 block">{t("labs.failure.stillUp")}</span>}
    </CalloutRow>
  );
}

/**
 * The auto-stop couldn't destroy this lab past its stop time (expired credentials, a removed
 * account…): it may still be billing, so it is said at the top of the page until it is gone.
 */
export function ReapFailedNotice({ title, error }: { title: string; error: CommandError }) {
  const t = useT();
  return (
    <CalloutRow tone="fail" title={t("labs.reapFailed.title", { title })}>
      <span className="block break-words font-mono text-[0.6875rem]">{describeError(error, t)}</span>
      <span className="mt-1 block text-foreground">{t("labs.reapFailed.hint")}</span>
    </CalloutRow>
  );
}
