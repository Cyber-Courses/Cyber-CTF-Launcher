"use client";

import { ExternalLink, RefreshCw, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { type ProviderStatus, type SystemReport } from "@/lib/tauri";
import { cantRun, providerLabel, localHypervisors } from "@/features/machine/hypervisors";
import { BusyLabel, Log } from "@/features/machine/setup-steps/parts";
import { getActions } from "@/features/machine/setup-steps/plan";
import { Choice, ChoiceAction, ChoiceGrid } from "@/components/ui/choice-card";
import { chosenHypervisor } from "@/features/machine/setup-steps/steps";
import { MachineSetupState } from "@/features/machine/setup-steps/use-machine-setup";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { openExternal } from "@/lib/failure";
import { confirmDestructive } from "@/lib/confirm";
import { useT, type T } from "@/lib/i18n";

/** A hypervisor's card note: installed (able to run here or not), else what the core's plan says
 *  gets it: the setup (installed for you), the player (this distribution), or its vendor. */
export function choiceNote(t: T, report: SystemReport, p: ProviderStatus) {
  if (p.hypervisor === true) return cantRun(report, p) ? t("machine.vm.cantRunHere") : t("machine.vm.installedHere");
  const get = getActions(report, p.provider).at(0);
  if (get?.kind === "manual") return get.reason === "distribution" ? t("machine.vm.installYourself") : t("machine.vm.vendorSite");
  return t("machine.vm.canInstall");
}

export function VmStep({ report, setup }: { report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const hypervisors = localHypervisors(report);
  // One hypervisor is enough; the pick lives in the setup state so the flow can wait for it.
  const choice = chosenHypervisor(report, setup);
  if (!choice) return <p className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.noHypervisor")}</p>;
  return (
    <div className="space-y-3">
      <ChoiceGrid>
        {hypervisors.map((p) => (
          <Choice
            key={p.provider}
            selected={p.provider === choice.provider}
            onSelect={() => {
              setup.setHypervisor(p.provider);
            }}
            mark={<HypervisorLogo provider={p.provider} />}
            title={providerLabel(p, report.os)}
            note={choiceNote(t, report, p)}
            badge={p.hypervisor === true ? "installed" : undefined}
          />
        ))}
      </ChoiceGrid>
      {/* Installed but unable to run (no /dev/kvm), or not before a new login (libvirt's group just
          joined): why, as the Machine page says. */}
      {(cantRun(report, choice) || getActions(report, choice.provider).at(0)?.kind === "relogin") && choice.reason && (
        <ChoiceAction>
          <span className="text-[0.8125rem] text-muted-foreground">
            {choice.reason.charAt(0).toUpperCase()}
            {choice.reason.slice(1)}.
          </span>
        </ChoiceAction>
      )}
      {/* Only when there is something to do: an installed hypervisor says so on its card. */}
      {choice.hypervisor !== true && <GetHypervisor choice={choice} report={report} setup={setup} />}
      <Log setup={setup} of={[choice.provider]} />
      <p className="pt-1 text-left text-[0.6875rem] leading-relaxed text-faint">{t("machine.vm.trademarks")}</p>
    </div>
  );
}

/** A hypervisor not installed, as the core's plan gets it: its one-click install, the restart
 *  that finishes turning it on (Hyper-V), or a link (its vendor's, or its own install on a
 *  distribution the setup doesn't install on yet) and a re-check. */
export function GetHypervisor({ choice, report, setup }: { choice: ProviderStatus; report: SystemReport; setup: MachineSetupState }) {
  const t = useT();
  const label = providerLabel(choice, report.os);
  const get = getActions(report, choice.provider).at(0);
  if (get?.kind === "restart") return <RestartToFinish name={label} reopens={report.wsl?.reopensAfterRestart ?? false} setup={setup} />;
  if (get?.kind === "install") {
    const dependency = get.dependency;
    return (
      <ChoiceAction>
        <span className="text-[0.8125rem] text-muted-foreground">{t("machine.vm.canInstallNamed", { name: label })}</span>
        <Button
          variant="primary"
          size="sm"
          onClick={() => {
            setup.install(choice.provider, dependency, t("machine.vm.installLog", { name: label }));
          }}
          disabled={!setup.canInstall}
        >
          <BusyLabel busy={setup.installing === choice.provider} label={t("machine.vm.installing")}>
            {t("machine.vm.install", { name: label })}
          </BusyLabel>
        </Button>
      </ChoiceAction>
    );
  }
  if (get?.kind !== "manual") return null;
  const download = get.url;
  return (
    <ChoiceAction>
      <span className="text-[0.8125rem] text-muted-foreground">
        {get.reason === "distribution" ? t("machine.body.distribution", { name: label }) : t("machine.vm.installThenRecheck", { name: label })}
      </span>
      <span className="flex shrink-0 gap-2">
        {download && (
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              openExternal(download);
            }}
          >
            <ExternalLink className="size-3.5" /> {t("machine.vm.get", { name: label })}
          </Button>
        )}
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            setup.onRefresh();
          }}
        >
          <RefreshCw className="size-3.5" /> {t("machine.vm.recheck")}
        </Button>
      </span>
    </ChoiceAction>
  );
}

/** Hyper-V turned on by the setup: Windows restarts to finish, once the player confirms in the
 *  system's own dialog (open apps close without saving), as WSL 2's restart does. */
function RestartToFinish({ name, reopens, setup }: { name: string; reopens: boolean; setup: MachineSetupState }) {
  const t = useT();
  const ask = () => {
    void confirmDestructive({
      title: t("machine.body.wsl.confirmTitle"),
      message: `${t("machine.body.wsl.confirmBody")} ${reopens ? t("machine.body.wsl.reopens") : t("machine.body.wsl.reopenByHand")}`,
      confirm: t("machine.body.wsl.restartNow"),
    }).then((yes) => {
      if (yes) setup.restart(t("machine.vm.restartReason", { name }), "hyperv");
    });
  };
  return (
    <div
      role="status"
      className="flex items-center gap-3 rounded-control bg-glass px-4 py-3 text-[0.8125rem] text-foreground shadow-[inset_0_0_0_1px_var(--border)]"
    >
      <StatusDot tone="warn" />
      <span className="min-w-0 flex-1">{setup.restarting ? t("machine.body.wsl.restarting") : t("machine.vm.restartToFinish", { name })}</span>
      <Button variant="primary" size="sm" className="shrink-0" disabled={setup.busy || setup.restarting} onClick={ask}>
        <RotateCcw className="size-3.5" /> {t("machine.body.wsl.restartNow")}
      </Button>
    </div>
  );
}
