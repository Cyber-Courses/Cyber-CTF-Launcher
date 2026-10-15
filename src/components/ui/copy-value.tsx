"use client";

import { useState } from "react";
import { Check, Copy } from "lucide-react";
import { cn } from "@/lib/utils";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** The clipboard: missing outside a secure context, though the DOM types say it's always there. */
const clipboard = (): Clipboard | undefined => navigator.clipboard;

/** Copies `text` on demand, then marks it copied for `showFor` ms; a failure is told under
 *  `failure`. The clipboard is missing outside a secure context: nothing happens then. */
export function useCopy(text: string, { showFor, failure }: { showFor: number; failure: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    clipboard()
      ?.writeText(text)
      .then(() => {
        setCopied(true);
        setTimeout(() => {
          setCopied(false);
        }, showFor);
      })
      .catch(tell(failure));
  };
  return { copied, copy };
}

/** A value with a one-click copy. Shows a check for a moment after copying. `label` is what is
 *  displayed when it should differ from the copied `text` (e.g. show `:56235`, copy the full
 *  `http://127.0.0.1:56235`). `title` replaces the "Copy <text>" tooltip (a masked secret must
 *  not show in it). */
export function CopyValue({ text, label, title, className }: { text: string; label?: string; title?: string; className?: string }) {
  const t = useT();
  const { copied, copy } = useCopy(text, { showFor: 1200, failure: t("errors.clipboard") });
  return (
    <button
      type="button"
      title={title ?? t("ui.copy.title", { text })}
      onClick={copy}
      className={cn(
        "inline-flex items-center gap-1.5 rounded-xs bg-glass px-1.5 py-0.5 font-mono text-[0.75rem] text-foreground shadow-[inset_0_0_0_1px_var(--border)] transition-colors hover:bg-glass-2",
        className,
      )}
    >
      <span className="break-all">{label ?? text}</span>
      {copied ? <Check className="size-3 text-success" /> : <Copy className="size-3 text-faint" />}
    </button>
  );
}
