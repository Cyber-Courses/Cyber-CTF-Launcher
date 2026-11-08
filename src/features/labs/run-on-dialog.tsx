"use client";

import { useRef } from "react";
import { useFocusTrap } from "@/lib/use-focus-trap";
import { useT } from "@/lib/i18n";

/**
 * A centered dialog over the page (Escape or a click on the backdrop closes it). The body
 * scrolls; `footer` (the actions) stays pinned at the bottom.
 */
export function RunOnDialog({ onClose, children, footer }: { onClose: () => void; children: React.ReactNode; footer: React.ReactNode }) {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);
  // Keep keyboard focus inside the dialog and restore it on close; Escape closes it.
  useFocusTrap(ref, true, onClose);
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--scrim)] p-4 backdrop-blur-[0.125rem]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        ref={ref}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-label={t("labs.runOn.dialogLabel")}
        className="surface-glass flex max-h-[85vh] w-full max-w-[28rem] flex-col rounded-panel outline-none"
      >
        <div className="min-h-0 flex-1 overflow-y-auto p-5">{children}</div>
        <div className="flex justify-end gap-2 border-t border-border px-5 py-3.5">{footer}</div>
      </div>
    </div>
  );
}
