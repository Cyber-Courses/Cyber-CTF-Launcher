"use client";

import { Button } from "@/components/ui/button";
import { HypervisorLogo } from "@/features/machine/hypervisor-logo";
import { cantRun, providerLabel, vagrantDrives } from "@/features/machine/hypervisors";
import { ProviderRow } from "@/features/machine/machine-parts";
import type { vmSetup } from "@/features/machine/machine-status";
import {
  FixButton,
  TESTED_TONE,
  RowTestButton,
  TestLine,
  UseDefaultButton,
  lowFor,
  testKey,
  useFreeHint,
  rowTest,
  type RowProps,
} from "@/features/machine/lab-row-parts";
import { getLastTest, setVmProvider } from "@/lib/settings";
import type { ProviderStatus, SystemReport } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

// The Machine page's hypervisors, one row per hypervisor installed here: the one VM labs run on
// (the default) with their last test, the others ready (their own test, on that hypervisor, and
// "Use by default"), blocked, or short of Vagrant or its plugin. Only what is installed: no row
// for one this machine lacks, "Add a provider" covers those.

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

export function HypervisorRows(props: Props) {
  const { vm } = props;
  const installed = vm.hypervisors.filter((p) => p.hypervisor === true);
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

/** The hypervisor labs run on: its last test (the VM labs' own), the test, and a server when
 *  memory is short. */
function RunsLabsRow({ p, state, vm, onUseServer, ...row }: Props & { p: ProviderStatus; state: "ready" | "testFailed" }) {
  const t = useT();
  const freeHint = useFreeHint("vm", row.memFree);
  const key = testKey("vm", p.provider);
  const last = getLastTest("vm", p.provider) ?? row.last;
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
          <TestLine test={last} now={row.now} />
        </>
      }
      hint={freeHint}
      actions={
        <>
          {lowFor("vm", row.memFree) && <ServerButton variant="ghost" onClick={onUseServer} />}
          <RowTestButton row={row} id={key} />
        </>
      }
      {...rowTest(row, key)}
      target={{ provider: p.provider, primary: true }}
    />
  );
}

/** Where an installed hypervisor that labs don't run on stands: "needs setup" exactly when the
 *  setup steps have something to install for it (`vagrantDrives`, the rule they read too). */
function standingOf(report: SystemReport, p: ProviderStatus): "cantRunHere" | "needsSetup" | "installed" {
  if (cantRun(report, p)) return "cantRunHere";
  if (!vagrantDrives(p)) return "needsSetup";
  return "installed";
}

/** An installed hypervisor labs don't run on: unable to run here (a server), short of Vagrant
 *  or its plugin (fix it), or ready (its own row below). */
function OtherHypervisorRow({ p, vm, onUseServer, ...row }: Props & { p: ProviderStatus }) {
  const t = useT();
  const { report, onFix } = row;
  const standing = standingOf(report, p);
  const common = {
    kind: "vm" as const,
    icon: <HypervisorLogo provider={p.provider} />,
    title: providerLabel(p, report.os),
    testing: false,
    onTestDone: () => {},
  };
  if (standing === "cantRunHere")
    return (
      <ProviderRow
        {...common}
        tone="warn"
        status={t("machine.screen.status.cantRunHere")}
        detail={p.reason ? `${p.reason.charAt(0).toUpperCase()}${p.reason.slice(1)}.` : ""}
        actions={<ServerButton variant="outline" onClick={onUseServer} />}
      />
    );
  if (standing === "needsSetup")
    return (
      <ProviderRow
        {...common}
        tone="warn"
        status={t("machine.screen.status.needsSetup")}
        detail={p.reason ? `${p.reason.charAt(0).toUpperCase()}${p.reason.slice(1)}.` : t("machine.screen.vm.vagrantMissing")}
        actions={<FixButton step="vagrant" provider={p.provider} onFix={onFix} />}
      />
    );
  return <IdleHypervisorRow p={p} vm={vm} {...row} />;
}

/** A ready hypervisor labs don't run on: its own test, on that hypervisor, its own last result,
 *  and "Use by default" (the Settings choice). */
function IdleHypervisorRow({ p, vm, ...row }: RowProps & { p: ProviderStatus; vm: Vm }) {
  const t = useT();
  const { report } = row;
  const key = testKey("vm", p.provider);
  return (
    <ProviderRow
      kind="vm"
      icon={<HypervisorLogo provider={p.provider} />}
      title={providerLabel(p, report.os)}
      tone="ok"
      status={t("machine.screen.status.installed")}
      detail={
        <>
          {vm.provider ? `${t("machine.screen.vm.installedIdle", { provider: providerLabel(vm.provider, report.os) })} · ` : ""}
          <TestLine test={getLastTest("vm", p.provider)} now={row.now} />
        </>
      }
      actions={
        <>
          <UseDefaultButton
            onClick={() => {
              setVmProvider(p.provider);
            }}
          />
          <RowTestButton row={row} id={key} />
        </>
      }
      {...rowTest(row, key)}
      target={{ provider: p.provider, primary: false }}
    />
  );
}
