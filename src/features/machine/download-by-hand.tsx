"use client";

import { useEffect, useRef, useState } from "react";
import { ExternalLink, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { openExternal } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import type { Interaction, SystemReport } from "@/lib/tauri";

/** A vendor's download a tool only comes by (an account, a licence): the vendor and its page. */
export type DownloadPage = Extract<Interaction, { kind: "downloadByHand" }>;

/** The download by hand among `asks`, if any. */
export const downloadOf = (asks: readonly Interaction[]): DownloadPage | undefined => asks.find((i): i is DownloadPage => i.kind === "downloadByHand");

/**
 * A tool only its vendor offers (VMware's, behind a Broadcom account): never left out. Its steps
 * (download it from the vendor's page, install it, come back), the page in the system's browser,
 * and Check again, which reads the machine again: while it reads, "Checking…"; read and still not
 * there, says so.
 */
export function DownloadByHand({ name, page, report, onCheck }: { name: string; page: DownloadPage; report: SystemReport; onCheck: () => unknown }) {
  const t = useT();
  const [state, setState] = useState<"idle" | "checking" | "notYet">("idle");
  // A new report while checking is the check's answer: shown at all, the tool is still missing.
  const seen = useRef(report);
  useEffect(() => {
    if (report === seen.current) return;
    seen.current = report;
    setState((s) => (s === "checking" ? "notYet" : s));
  }, [report]);
  return (
    <div className="space-y-2" data-testid="download-by-hand">
      <p className="text-[0.8125rem] text-muted-foreground">{t("machine.asks.byHand.steps", { vendor: page.vendor, name })}</p>
      <span className="flex flex-wrap gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            openExternal(page.url);
          }}
        >
          <ExternalLink className="size-3.5" /> {t("machine.asks.byHand.open", { vendor: page.vendor })}
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={state === "checking"}
          onClick={() => {
            setState("checking");
            onCheck();
          }}
        >
          <RefreshCw className="size-3.5" /> {t("machine.asks.byHand.check")}
        </Button>
      </span>
      {state !== "idle" && (
        <p role="status" className="text-[0.75rem] text-muted-foreground">
          {state === "checking" ? t("machine.asks.byHand.checking") : t("machine.asks.byHand.notYet", { name })}
        </p>
      )}
    </div>
  );
}
