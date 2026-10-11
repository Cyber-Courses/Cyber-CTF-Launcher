"use client";

import { useEffect, useState } from "react";
import { Markdown } from "@/components/ui/markdown";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { apiQuery } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** The lab's brief (markdown from CyberBackend). */
export function LabBrief({ labId }: { labId: string }) {
  const t = useT();
  const [content, setContent] = useState<string | null | undefined>(undefined);
  useEffect(() => {
    apiQuery<{ labs: { contentMd: string | null }[] }>("labBrief", { id: labId })
      .then((d) => setContent(d.labs[0]?.contentMd ?? null))
      .catch(() => setContent(null));
  }, [labId]);
  return (
    <Panel>
      <PanelHeader title={t("labs.brief.title")} />
      <div className="p-4">
        {content === undefined ? (
          <div className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
            <Spinner className="size-3.5" /> {t("labs.brief.loading")}
          </div>
        ) : content ? (
          <Markdown content={content} className="space-y-3 text-[0.8125rem] leading-relaxed text-foreground" />
        ) : (
          <p className="text-[0.8125rem] text-muted-foreground">{t("labs.brief.empty")}</p>
        )}
      </div>
    </Panel>
  );
}
