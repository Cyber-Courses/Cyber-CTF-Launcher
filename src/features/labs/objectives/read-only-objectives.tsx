"use client";

import { Badge } from "@/components/ui/badge";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { PROSE } from "@/components/ui/screen-width";
import type { LabChallenge } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/**
 * A lab's challenges as its own `.ctf/objectives.json` lists them, to read only: a lab imported
 * from a folder isn't published, so Cyber CTF has no answers to check and no progress to keep for
 * it, and the panel says so. Each challenge with what it asks and its guided steps; never an
 * answer (the core never reads them out of the file) nor a hint's text.
 */
export function ReadOnlyObjectives({ challenges }: { challenges: LabChallenge[] }) {
  const t = useT();
  return (
    <Panel>
      <PanelHeader title={t("labs.objectives.title")} meta={t("labs.objectives.count", { count: challenges.length })} />
      <p className="border-b border-border px-4 py-2.5 text-[0.75rem] text-muted-foreground">{t("labs.objectives.readOnly")}</p>
      <ol>
        {challenges.map((c) => (
          <li key={c.key} className="space-y-2 border-t border-border px-4 py-3 first:border-t-0">
            <div className="flex items-center gap-2.5">
              <span className="min-w-0 flex-1 truncate text-[0.8125rem] font-medium text-foreground">{c.title}</span>
              {c.optional && <Badge variant="outline">{t("labs.objectives.optional")}</Badge>}
            </div>
            {c.prompt && <p className={cn("text-[0.8125rem] leading-relaxed whitespace-pre-line text-muted-foreground", PROSE)}>{c.prompt}</p>}
            {c.steps.length > 0 && (
              <div className="space-y-1.5">
                <p className="section-label">{t("labs.objectives.steps")}</p>
                <ol className="list-decimal space-y-1 pl-5 text-[0.75rem] text-muted-foreground">
                  {c.steps.map((s) => (
                    <li key={s.key}>
                      <span className="text-foreground">{s.title}</span>
                      {s.prompt && <span className={cn("block whitespace-pre-line", PROSE)}>{s.prompt}</span>}
                    </li>
                  ))}
                </ol>
              </div>
            )}
          </li>
        ))}
      </ol>
    </Panel>
  );
}
