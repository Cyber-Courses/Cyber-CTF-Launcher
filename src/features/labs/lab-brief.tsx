"use client";

import { useEffect, useState } from "react";
import { Markdown } from "@/components/ui/markdown";
import { Panel, PanelHeader } from "@/components/ui/panel";
import { Spinner } from "@/components/ui/spinner";
import { PROSE } from "@/components/ui/screen-width";
import { apiQuery } from "@/lib/tauri";
import { cn } from "@/lib/class-names";
import { useT } from "@/lib/i18n";
import { warn } from "@/lib/failure";
import { describeError } from "@/lib/errors";

/** The brief as read: its markdown (null: the lab has none), or why it couldn't be read. */
type BriefRead = { content: string | null } | { error: string };

/** The lab's brief (markdown from CyberBackend). A failed read is said as one, never as a lab
 *  without a brief. */
export function LabBrief({ labId }: { labId: string }) {
  const t = useT();
  // The read and the lab it is for: the page can move to another lab while one is read, and a
  // read for a lab no longer shown is never shown (the brief shows as loading until its own).
  const [answer, setAnswer] = useState<{ labId: string; read: BriefRead } | undefined>(undefined);
  const read = answer?.labId === labId ? answer.read : undefined;
  useEffect(() => {
    // An answer for a lab no longer shown is dropped, so a slow one never replaces the brief of
    // the lab now shown.
    let shown = true;
    apiQuery<{ labs: { contentMd: string | null }[] }>("labBrief", { id: labId })
      .then((d) => {
        if (shown) setAnswer({ labId, read: { content: d.labs[0]?.contentMd ?? null } });
      })
      .catch((e: unknown) => {
        warn("reading the lab's brief")(e);
        if (shown) setAnswer({ labId, read: { error: describeError(e) } });
      });
    return () => {
      shown = false;
    };
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
