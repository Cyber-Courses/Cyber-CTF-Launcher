"use client";

import { useRef } from "react";
import { AlertTriangle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useFocusTrap } from "@/lib/use-focus-trap";
import { useT } from "@/lib/i18n";

/**
 * Asks before a destructive action. Cancel comes first and takes the focus, so Enter or Escape
 * right after a misclick keeps everything; a click on the backdrop cancels too.
 */
export function ConfirmDialog({
  title,
  children,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  title: string;
  children: React.ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);
  useFocusTrap(ref, true, onCancel);

  return (
    <div
      className="animate-fade-in fixed inset-0 z-50 flex items-center justify-center bg-[var(--scrim)] p-4 backdrop-blur-[2px]"
      onMouseDown={(e) => e.target === e.currentTarget && onCancel()}
    >
      <div
        ref={ref}
        tabIndex={-1}
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="confirm-title"
        aria-describedby="confirm-body"
        className="surface-glass w-full max-w-[26rem] rounded-[1rem] p-5 outline-none"
      >
        <div className="flex items-start gap-3">
          <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-control bg-destructive/10 text-destructive shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--destructive)_30%,transparent)]">
            <AlertTriangle className="size-4" />
          </span>
          <div className="min-w-0">
            <p id="confirm-title" className="text-[0.9375rem] font-medium text-foreground">
              {title}
            </p>
            <div id="confirm-body" className="mt-1 text-[0.8125rem] leading-relaxed text-muted-foreground">
              {children}
            </div>
          </div>
        </div>
        <div className="mt-5 flex justify-end gap-2">
          <Button variant="ghost" size="sm" onClick={onCancel}>
            {t("ui.confirm.cancel")}
          </Button>
          <Button variant="destructive" size="sm" onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
