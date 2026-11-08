"use client";

import { Button } from "@/components/ui/button";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { cantRun, providerLabel } from "@/features/machine/hypervisors";
import { ProviderRow } from "@/features/machine/machine-parts";
import type { vmSetup } from "@/features/machine/machine-status";
import { FixButton, TESTED_TONE, TestButton, TestLine, lowFor, useFreeHint, type RowProps } from "@/features/machine/lab-row-parts";
import type { ProviderStatus, SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

// The Machine page's hypervisors, one row each: the one VM labs run on with their last test,
// the others installed (ready, blocked, or short of Vagrant or its plugin), and the ones this OS
// could have; or, with none that applies here, the way to a server.

type Vm = ReturnType<typeof vmSetup>;
type Props = RowProps & { vm: Vm; onUseServer: () => void };

/** The server button, where VM labs can't run here or memory is short. */
function ServerButton({ variant, onClick }: { variant: "outline" | "ghost"; onClick: () => void }) {
  const t = useT();
  return (
    <Button variant={variant} size="xs" onClick={onClick}>
      {t("machine.screen.useServer")}
    </Button>
  );
}

/** The icon tile and the way to a hypervisor's setup, for the rows that stand in for a list. */
function PlaceholderRow({ status, detail, actions }: { status: string; detail: string; actions: React.ReactNode }) {
  const t = useT();
  return (
    <ProviderRow
      kind="vm"
      icon={<HypervisorLogo provider={undefined} />}
      title={t("machine.steps.vm.title")}
      tone="muted"
      status={status}
      detail={detail}
      actions={actions}
      testing={false}
      onTestDone={() => {}}
    />
  );
}

/** Every hypervisor installed here, each with its own state, the one labs run on marked; with
 *  none installed, one row to add one, and with none that applies here, the way to a server.
 *  Hypervisors not installed stay out: "Add a provider" covers those. */
export function HypervisorRows(props: Props) {
  const t = useT();
  const { vm, onUseServer, onFix } = props;
  if (vm.state === "notApplicable")
    return (
      <PlaceholderRow
        status={t("machine.screen.status.notOnThisMachine")}
        detail={t("machine.screen.vm.notApplicable")}
        actions={<ServerButton variant="outline" onClick={onUseServer} />}
      />
    );
  const installed = vm.hypervisors.filter((p) => p.hypervisor === true);
  if (installed.length === 0)
    return (
      <PlaceholderRow status={t("machine.screen.status.notSetUp")} detail={t("machine.screen.vm.none")} actions={<FixButton step="vm" onFix={onFix} add />} />
    );
  return (
    <>
      {installed.map((p) =>
        p.provider === vm.provider?.provider && (vm.state === "ready" || vm.state === "testFailed") ? (
          <RunsLabsRow key={p.provider} p={p} state={vm.state} {...props} />
        ) : (
          <OtherHypervisorRow key={p.provider} p={p} {...props} />
        ),
      )}
    </>
  );
}

/** The hypervisor labs run on: its last test, the test, and a server when memory is short. */
function RunsLabsRow({ p, state, vm, onUseServer, ...row }: Props & { p: ProviderStatus; state: "ready" | "testFailed" }) {
  const t = useT();
  const freeHint = useFreeHint("vm", row.memFree);
  return (
    <ProviderRow
      kind="vm"
      icon={<HypervisorLogo provider={p.provider} />}
      title={providerLabel(p, row.report.os)}
      badge={t("machine.screen.badge.default")}
      tone={TESTED_TONE[state]}
      status={t(`machine.screen.status.${state}`)}
      detail={
        <>
          {vm.automatic ? `${t("machine.screen.vm.automatic")} · ` : ""}
          <TestLine test={row.last} now={row.now} />
        </>
      }
      hint={freeHint}
      actions={
        <>
          {lowFor("vm", row.memFree) && <ServerButton variant="ghost" onClick={onUseServer} />}
          <TestButton testing={row.testing} onClick={row.onToggleTest} />
        </>
      }
      testing={row.testing}
      onTestDone={row.onTestDone}
    />
  );
}

/** Where an installed hypervisor that labs don't run on stands. */
function standingOf(p: ProviderStatus, report: SystemReport): "cantRunHere" | "needsSetup" | "installed" {
  if (cantRun(p, report)) return "cantRunHere";
  if (!report.vagrant.installed || (p.plugin && !p.pluginInstalled)) return "needsSetup";
  return "installed";
}

/** An installed hypervisor labs don't run on: unable to run here (a server), short of Vagrant
 *  or its plugin (fix it), or idle. */
function OtherHypervisorRow({ p, vm, onUseServer, report, onFix }: Props & { p: ProviderStatus }) {
  const t = useT();
  const standing = standingOf(p, report);
  const reason = p.reason ? `${p.reason.charAt(0).toUpperCase()}${p.reason.slice(1)}.` : "";
  const detail = {
    cantRunHere: reason,
    needsSetup: t("machine.screen.vm.vagrantMissing"),
    installed: vm.provider ? t("machine.screen.vm.installedIdle", { provider: providerLabel(vm.provider, report.os) }) : t("machine.screen.detail.installed"),
  }[standing];
  const action = {
    cantRunHere: <ServerButton variant="outline" onClick={onUseServer} />,
    needsSetup: <FixButton step="vagrant" onFix={onFix} />,
    installed: null,
  }[standing];
  return (
    <ProviderRow
      kind="vm"
      icon={<HypervisorLogo provider={p.provider} />}
      title={providerLabel(p, report.os)}
      tone={standing === "installed" ? "ok" : "warn"}
      status={t(`machine.screen.status.${standing}`)}
      detail={detail}
      actions={action}
      testing={false}
      onTestDone={() => {}}
    />
  );
}
