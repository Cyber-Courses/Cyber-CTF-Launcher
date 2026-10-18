"use client";

import { Button } from "@/components/ui/button";
import { CopyValue } from "@/components/ui/copy-value";
import { Select } from "@/components/ui/input";
import { KeyValue, Panel, PanelHeader } from "@/components/ui/panel";
import { AutoStop } from "@/features/labs/lab-timers";
import { RerunSetupLabel } from "@/features/labs/lab-detail/deployment-panel";
import { bindHost, publishedBinds } from "@/features/labs/lab-detail/lab-state";
import type { LabStatus } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** A running lab's details: where it runs, its address or published ports, and (VM labs) the
 *  setup to run again. */
export function DetailsPanel({
  status,
  engine,
  meta,
  cost = null,
  provision,
}: {
  status: LabStatus | undefined;
  engine: string | null;
  meta: string;
  /** A cloud lab's estimated cost, when it gives one. */
  cost?: string | null;
  /** VM labs whose setup can run again in place. */
  provision: SetupProps | null;
}) {
  const t = useT();
  const url = status?.url;
  const binds = publishedBinds(status?.machines ?? []);
  const here = status?.host ?? t("labs.detail.thisMachine");
  return (
    <Panel>
      <PanelHeader title={t("labs.detail.details")} meta={meta} />
      <div>
        <KeyValue k={t("labs.detail.runsOn")}>{here}</KeyValue>
        {engine && !status?.host && <KeyValue k={t("labs.detail.engine")}>{engine}</KeyValue>}
        {status?.expiresAt && (
          <KeyValue k={t("labs.detail.autoStop")}>
            <AutoStop at={status.expiresAt} />
          </KeyValue>
        )}
        {cost && <KeyValue k={t("labs.detail.estimatedCost")}>{cost}</KeyValue>}
        {!binds.length && url && (
          <KeyValue k={t("labs.detail.address")}>
            <CopyValue text={url} />
          </KeyValue>
        )}
      </div>
      {/* The bind spelled out: which service inside the lab answers on which address of this
          machine, with that local address one click away. */}
      {binds.length > 0 && (
        <div className="border-t border-border px-4 py-3">
          <div className="mb-2 flex justify-between gap-3">
            <span className="section-label">{t("labs.detail.insideLab")}</span>
            <span className="section-label">{t("labs.detail.onHost", { where: here })}</span>
          </div>
          <div className="space-y-1.5">
            {binds.map((b) => (
              <div key={`${b.machine}:${String(b.target)}`} className="flex items-center justify-between gap-3">
                <span className="min-w-0 truncate font-mono text-[0.75rem] text-muted-foreground">
                  {b.machine} :{b.target}
                </span>
                <CopyValue text={`http://${bindHost(url)}:${String(b.published)}`} label={`:${String(b.published)}`} />
              </div>
            ))}
          </div>
        </div>
      )}
      {provision && <SetupSection {...provision} machines={(status?.machines ?? []).filter((m) => !m.infra).map((m) => m.name)} />}
    </Panel>
  );
}

type SetupProps = {
  /** Which machine to provision again ("" = all). */
  target: string;
  onTarget: (machine: string) => void;
  busy: boolean;
  running: boolean;
  onRun: () => void;
};

/** Runs a VM lab's setup (its provisioners) again, on one machine or all, in place. */
function SetupSection({ target, onTarget, busy, running, onRun, machines }: SetupProps & { machines: string[] }) {
  const t = useT();
  return (
    <div className="space-y-2.5 border-t border-border p-4">
      <p className="section-label">{t("labs.detail.setup")}</p>
      <p className="text-[0.75rem] leading-relaxed text-muted-foreground">{t("labs.detail.setupDescription")}</p>
      <Select
        fieldSize="sm"
        aria-label={t("labs.detail.provisionTarget")}
        value={target}
        onChange={(e) => {
          onTarget(e.target.value);
        }}
        disabled={busy}
      >
        <option value="">{t("labs.detail.allMachines")}</option>
        {machines.map((name) => (
          <option key={name} value={name}>
            {name}
          </option>
        ))}
      </Select>
      <Button variant="outline" size="sm" className="w-full" onClick={onRun} disabled={busy} title={t("labs.detail.rerunHint")}>
        <RerunSetupLabel running={running} />
      </Button>
    </div>
  );
}
