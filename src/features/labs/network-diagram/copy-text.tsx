"use client";

import { Check, Copy } from "lucide-react";
import { useCopy } from "@/components/ui/copy-value";
import { useT } from "@/lib/i18n";

/** Click-to-copy wrapper (addresses). Stops React Flow from dragging the node. */
export function CopyText({ text, children }: { text: string; children: React.ReactNode }) {
  const t = useT();
  const { copied, copy } = useCopy(text, { showFor: 1000, failure: t("diagram.copy.failed") });
  return (
    <span
      className="copy-chip"
      title={t("diagram.copy.title", { text })}
      onMouseDown={(e) => {
        e.stopPropagation();
      }}
      onClick={(e) => {
        e.stopPropagation();
        copy();
      }}
    >
      {children}
      {copied ? <Check size={11} className="copy-ok" /> : <Copy size={11} className="copy-ico" />}
    </span>
  );
}
