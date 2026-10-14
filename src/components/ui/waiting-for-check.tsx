"use client";

import { Spinner } from "@/components/ui/spinner";
import { CheckFailed } from "@/features/machine/check-failed";

/** Shown where the machine check is needed and hasn't answered: the loader, or why it failed
 *  with Try again (the same view the main window shows). */
export function WaitingForCheck({ what, checkError, onRetry }: { what: string; checkError: string | null; onRetry: () => unknown }) {
  return checkError ? (
    <CheckFailed error={checkError} onRetry={onRetry} />
  ) : (
    <p className="flex items-center gap-2.5 text-[0.8125rem] text-muted-foreground">
      <Spinner className="size-3.5" /> {what}
    </p>
  );
}
