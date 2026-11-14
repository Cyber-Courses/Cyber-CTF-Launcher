"use client";

import { useId, useRef } from "react";
import { ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useFocusTrap } from "@/lib/use-focus-trap";

/** What the dialog asks and the two answers it offers. */
type ConsentDialogProps = {
  title: string;
  children: React.ReactNode;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  /** The confirm button is disabled (what it needs isn't there). */
  confirmDisabled?: boolean;
};

/**
 * Asks the player to agree to something that trusts or reaches beyond the lab (a server's
 * certificate, their cloud credentials, this computer): what it is, in full, then Cancel (first,
 * and focused, so Enter or Escape right away agrees to nothing) and the agreement. Keyboard: Tab
 * stays in the dialog, Escape cancels; a click on the backdrop cancels too.
 */
export function ConsentDialog({ title, children, confirmLabel, cancelLabel, onConfirm, onCancel, confirmDisabled = false }: ConsentDialogProps) {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const bodyId = useId();
  useFocusTrap(ref, true, onCancel);
  return (
    <div
      className="animate-fade-in fixed inset-0 z-50 flex items-center justify-center bg-[var(--scrim)] p-4 backdrop-blur-[2px]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onCancel();
      }}
    >
      <div
        ref={ref}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
        className="surface-glass max-h-[90vh] w-full max-w-[34rem] overflow-y-auto rounded-[1rem] p-5 outline-none"
      >
        <div className="flex items-start gap-3">
          <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-control bg-warning/10 text-warning">
            <ShieldAlert className="size-4" aria-hidden="true" />
          </span>
          <div className="min-w-0 flex-1">
            <h2 id={titleId} className="text-[0.9375rem] font-medium text-foreground">
              {title}
            </h2>
            <div id={bodyId} className="mt-1 text-[0.8125rem] leading-relaxed text-muted-foreground">
              {children}
            </div>
          </div>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={onCancel}>
            {cancelLabel}
          </Button>
          <Button variant="default" size="sm" onClick={onConfirm} disabled={confirmDisabled}>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
