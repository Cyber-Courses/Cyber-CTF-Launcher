"use client";

import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** Click-to-copy wrapper (addresses). Stops React Flow from dragging the node. */
export function CopyText({ text, children }: { text: string; children: React.ReactNode }) {
  const t = useT();
  const [copied, setCopied] = useState(false);
  return (
    <span
      className="copy-chip"
      title={t("diagram.copy.title", { text })}
      onMouseDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        navigator.clipboard
          ?.writeText(text)
          .then(() => {
            setCopied(true);
            setTimeout(() => setCopied(false), 1000);
          })
          .catch(tell(t("diagram.copy.failed")));
      }}
    >
      {children}
      {copied ? <Check size={11} className="copy-ok" /> : <Copy size={11} className="copy-ico" />}
    </span>
  );
}
