"use client";

import { useEffect, useState } from "react";
import { Markdown } from "@/components/ui/markdown";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { PROSE } from "@/components/ui/screen-width";
import { apiQuery } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { warn } from "@/lib/failure";
import { describeError } from "@/lib/errors";

/** The brief as read: its markdown (null: the lab has none), or why it couldn't be read. */
type BriefRead = { content: string | null } | { error: string };

/** The lab's brief (markdown from CyberBackend). A failed read is said as one, never as a lab
 *  without a brief. */
export function LabBrief({ labId }: { labId: string }) {
  const t = useT();
  const [read, setRead] = useState<BriefRead | undefined>(undefined);
  useEffect(() => {
    apiQuery<{ labs: { contentMd: string | null }[] }>("labBrief", { id: labId })
      .then((d) => {
        setRead({ content: d.labs[0]?.contentMd ?? null });
      })
      .catch((e: unknown) => {
        warn("reading the lab's brief")(e);
        setRead({ error: describeError(e) });
      });
  }, [labId]);
  return (
    <Panel>
      <PanelHeader title={t("labs.brief.title")} />
      <div className="p-4">
        {read === undefined ? (
          <div className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
            <Spinner className="size-3.5" /> {t("labs.brief.loading")}
          </div>
        ) : "error" in read ? (
          <p className="text-[0.8125rem] text-muted-foreground">{t("labs.brief.readFailed", { why: read.error })}</p>
        ) : read.content ? (
          <Markdown content={read.content} className={cn("space-y-3 text-[0.8125rem] leading-relaxed text-foreground", PROSE)} />
        ) : (
          <p className="text-[0.8125rem] text-muted-foreground">{t("labs.brief.empty")}</p>
        )}
      </div>
    </Panel>
  );
}
