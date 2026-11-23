"use client";

import { Switch } from "@/components/ui/switch";
import { PortChoice } from "@/features/labs/port-mode-prompt";
import type { RunTargetChoice } from "@/features/labs/lab-detail/use-run-target";
import { useT } from "@/lib/i18n";

/** The container path's last step: the host ports this machine publishes the lab's services on.
 *  (The engine, when more than one answers, is its own step before this one.) */
export function PortsStep({ title, run }: { title: string; run: RunTargetChoice }) {
  const t = useT();
  return (
    <div className="space-y-4">
      <div>
        <p className="serif-title text-[1.5rem] text-foreground">{t("labs.detail.portsHere")}</p>
        <p className="mt-1 text-[0.8125rem] text-muted-foreground">{title}</p>
      </div>
      <PortChoice value={run.ports} onChange={run.setPorts} />
    </div>
  );
}

/** "Don't ask again": saves the path's choices (engine + ports, or the VM form and its hypervisor)
 *  on Start, so the next launch of a lab here skips straight to it. */
export function RememberToggle({ run }: { run: RunTargetChoice }) {
  const t = useT();
  return (
    <label className="mt-4 mb-1 flex items-center gap-2.5 px-1 text-[0.8125rem] text-muted-foreground">
      <Switch checked={run.dontAskAgain.checked} onCheckedChange={run.dontAskAgain.set} aria-label={t("labs.runOn.rememberChoice")} />
      {t("labs.runOn.rememberChoice")}
    </label>
  );
}
