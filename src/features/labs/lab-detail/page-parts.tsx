"use client";

import type { ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Back to the list, with its Esc shortcut. */
export function BackLink({ onBack }: { onBack: () => void }) {
  const t = useT();
  return (
    <button
      type="button"
      onClick={onBack}
      className="group -mb-1 inline-flex items-center gap-1.5 text-[0.75rem] text-muted-foreground transition-colors hover:text-foreground"
    >
      <ArrowLeft className="size-3.5" /> {t("labs.detail.allLabs")}
      <kbd className="kbd">Esc</kbd>
    </button>
  );
}

/** The question the lab's evidence answers. */
export function ObjectivePanel({ question }: { question: string }) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("labs.detail.objective")} meta={t("labs.detail.evidence")} />
      <p className="px-4 py-3.5 text-[0.8125rem] leading-relaxed text-foreground">{question}</p>
    </Panel>
  );
}

/** A panel kept in place while the lab is being removed: dimmed and out of reach (inert), so
 *  the page doesn't jump, and back as it was should the removal fail. */
export function Dimmed({ on, children }: { on: boolean; children: ReactNode }) {
  return (
    <div className={cn("transition-opacity", on && "pointer-events-none opacity-50")} inert={on || undefined} aria-busy={on || undefined}>
      {children}
    </div>
  );
}
