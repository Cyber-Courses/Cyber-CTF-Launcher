"use client";

import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { MAIN_PROVIDERS } from "@/features/cloud/cloud-model";
import { ProviderButtons } from "@/features/cloud/cloud-panels";
import type { CloudProvider } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** No account connected (none yet, or the last one removed): what the page is for, a button per
 *  main provider that opens its setup, and the setup's own provider choice for the others. */
export function FirstRun({ onSetup, onConnect }: { onSetup: () => void; onConnect: (provider: CloudProvider) => void }) {
  const t = useT();
  return (
    <div className="surface-panel flex flex-col items-center rounded-panel px-6 py-10 text-center">
      <h3 className="serif-title text-[1.5rem] text-foreground">{t("cloud.firstRun.title")}</h3>
      <p className="mt-1.5 max-w-sm text-[0.875rem] text-muted-foreground">{t("cloud.firstRun.body")}</p>
      <div className="mt-5 w-full max-w-md overflow-hidden rounded-control bg-glass shadow-[inset_0_0_0_1px_var(--border)]">
        <Step n={1} title={t("cloud.firstRun.steps.connect.title")} body={t("cloud.firstRun.steps.connect.body")} />
        <Step n={2} title={t("cloud.firstRun.steps.pick.title")} body={t("cloud.firstRun.steps.pick.body")} />
        <Step n={3} title={t("cloud.firstRun.steps.stop.title")} body={t("cloud.firstRun.steps.stop.body")} />
      </div>
      <section aria-label={t("cloud.connect.title")} className="mt-5 flex flex-col items-center gap-2.5">
        <h4 className="text-[0.8125rem] font-medium text-muted-foreground">{t("cloud.connect.title")}</h4>
        <ProviderButtons providers={MAIN_PROVIDERS} onConnect={onConnect} />
      </section>
      <Button size="sm" variant="ghost" className="mt-3" onClick={onSetup}>
        <Plus className="size-3.5" /> {t("cloud.screen.setUp")}
      </Button>
    </div>
  );
}

/** A numbered step row (the mockup's `.st`): a ringed number, the title, then a note. */
function Step({ n, title, body }: { n: number; title: string; body: string }) {
  return (
    <div className="grid grid-cols-[1.25rem_minmax(0,1fr)] items-start gap-3 border-t border-border px-4 py-2.5 text-left first:border-t-0">
      <span className="mt-px grid size-[1.1rem] place-items-center rounded-full font-mono text-[0.625rem] text-muted-foreground tabular-nums shadow-[inset_0_0_0_1px_var(--input)]">
        {n}
      </span>
      <div className="min-w-0">
        <p className="text-[0.8125rem] font-medium text-foreground">{title}</p>
        <p className="text-[0.75rem] text-muted-foreground">{body}</p>
      </div>
    </div>
  );
}
