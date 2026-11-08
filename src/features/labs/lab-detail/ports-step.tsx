"use client";

import { Segmented } from "@/components/ui/segmented";
import { Switch } from "@/components/ui/switch";
import { PortChoice } from "@/features/labs/port-mode-prompt";
import { engineName } from "@/features/machine/setup-steps/engines";
import type { RunTargetChoice } from "@/features/labs/lab-detail/use-run-target";
import { useT } from "@/lib/i18n";

/** Step 2 for Containers: the engine to run on (when more than one answers) and the host ports. */
export function PortsStep({ title, run }: { title: string; run: RunTargetChoice }) {
  const t = useT();
  return (
    <div className="space-y-4">
      <div>
        <p className="serif-title text-[1.5rem] text-foreground">{t("labs.runOn.formHeading")}</p>
        <p className="mt-1 text-[0.8125rem] text-muted-foreground">{title}</p>
      </div>
      {run.engines.length > 1 && run.engine && (
        <div className="space-y-1.5">
          <p className="section-label">{t("labs.runOn.engine")}</p>
          <Segmented
            label={t("labs.runOn.engine")}
            value={run.engine}
            onChange={run.pickEngine}
            options={run.engines.map((e) => ({ value: e, label: engineName(e) }))}
          />
        </div>
      )}
      <div className="space-y-1.5">
        <p className="section-label">{t("labs.detail.portsHere")}</p>
        <PortChoice value={run.ports} onChange={run.setPorts} />
      </div>
      <RememberToggle run={run} />
    </div>
  );
}

/** "Don't ask again": saves the step's choice (engine + ports, or the VM form) on Start, so the
 *  next launch of a lab here skips straight to it. */
export function RememberToggle({ run }: { run: RunTargetChoice }) {
  const t = useT();
  return (
    <label className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
      <Switch checked={run.remember} onCheckedChange={run.setRemember} aria-label={t("labs.runOn.rememberChoice")} />
      {t("labs.runOn.rememberChoice")}
    </label>
  );
}
