import { useEffect, useRef, useState } from "react";
import { type SelfTestResult } from "@/features/machine/self-test";
import { getVmProvider, setVmProvider } from "@/lib/settings";
import type { Provider } from "@/lib/tauri";
import {
  installDependency,
  installVagrantPlugin,
  machineRestart,
  machineSelftestPrefetch,
  type Dependency,
  type DockerEngine,
  type SystemReport,
  type WindowsFeature,
} from "@/lib/tauri";
import { hasHypervisor, isDockerReady } from "@/features/machine/setup-steps/steps";
import { useRunLog } from "@/lib/use-run-log";
import { ignore, tell } from "@/lib/failure";
import { describeError } from "@/lib/errors";
import { translate } from "@/lib/i18n";
import { useToolInstallsOff } from "@/lib/use-managed";

/** Starts each self-test's download in the background as soon as the machine can run it. */
function usePrefetch(report: SystemReport | null) {
  const prefetched = useRef({ docker: false, vm: false });
  const dockerReady = isDockerReady(report);
  const vmReady = hasHypervisor(report) && !!report?.vagrant.installed;
  useEffect(() => {
    if (dockerReady && !prefetched.current.docker) {
      prefetched.current.docker = true;
      machineSelftestPrefetch("docker", null).catch(ignore("a head start only; the self-test pulls the image when it runs"));
    }
    if (vmReady && !prefetched.current.vm) {
      prefetched.current.vm = true;
      machineSelftestPrefetch("vm", getVmProvider()).catch(ignore("a head start only; the self-test pulls the box when it runs"));
    }
  }, [dockerReady, vmReady]);
}

/** The installs the steps run (a tool, a Vagrant plugin), one at a time, with their log, each
 *  followed by a new machine check. */
function useInstalls(onRefresh: () => void) {
  const [installing, setInstalling] = useState<string | null>(null);
  const [installerOpened, setInstallerOpened] = useState(false);
  // "Turn on WSL 2" finished here: Windows needs a restart, whatever the last check said.
  const [wslTurnedOn, setWslTurnedOn] = useState(false);
  const { lines, run: runLog } = useRunLog();
  // What the log is about (an install id or a plugin): each step shows only its own.
  const [logsFor, setLogsFor] = useState<string | null>(null);
  // Why each install that failed in this flow failed, by its id, until it is run again: its row
  // says so instead of reading as still to do (or, worse, as done).
  const [failures, setFailures] = useState<Partial<Record<string, string>>>({});

  /** Runs install `id`, its log starting at `first`; a failure ends the log with "✗ <why>" and
   *  is kept as the install's failure. */
  async function run(id: string, first: string, task: (onLog: (line: string) => void) => Promise<void>) {
    setInstalling(id);
    setLogsFor(id);
    setFailures((f) => ({ ...f, [id]: undefined }));
    await runLog(first, async (onLog) => {
      try {
        await task(onLog);
      } catch (e) {
        setFailures((f) => ({ ...f, [id]: describeError(e) }));
        throw e;
      }
    });
    setInstalling(null);
    onRefresh();
  }

  const installsOff = useToolInstallsOff();
  const install = (id: string, dep: Dependency, first: string) => {
    if (installsOff) return;
    void run(id, first, async (onLog) => {
      await installDependency(dep, onLog);
      if (dep === "docker") setInstallerOpened(true);
      if (dep === "wsl") setWslTurnedOn(true);
    });
  };
  /** Adds the Vagrant plugin a hypervisor needs (e.g. vagrant-vmware-desktop). */
  const installPlugin = (plugin: string) => {
    if (installsOff) return;
    void run(plugin, translate("machine.vm.pluginLog", { plugin }), (onLog) => installVagrantPlugin(plugin, onLog));
  };
  /** No install runs, and the policy allows one. */
  const canInstall = installing === null && !installsOff;
  /** Why install `id` failed in this flow, if it did (and wasn't run again since). */
  const failure = (id: string): string | null => failures[id] ?? null;
  return { installing, installsOff, canInstall, installerOpened, wslTurnedOn, logs: lines ?? [], logsFor, install, installPlugin, failure };
}

/** Windows counts down to the restart that finishes WSL 2 (or Hyper-V): kept with the flow, not
 *  the step, so a remounted step (the onboarding draws each step afresh) can't ask for it a second
 *  time. */
function useRestart() {
  const [restarting, setRestarting] = useState(false);
  /** Restarts Windows to finish turning `feature` on; `reason` is the note Windows shows. */
  const restart = (reason: string, feature: WindowsFeature) => {
    if (restarting) return;
    setRestarting(true);
    machineRestart(reason, feature).catch((e: unknown) => {
      setRestarting(false);
      tell(translate("machine.body.wsl.restartFailed"))(e);
    });
  };
  return { restarting, restart };
}

/** Install and test state for the setup steps. One per flow, passed to every step body. */
export function useMachineSetup(report: SystemReport | null, onRefresh: () => unknown) {
  usePrefetch(report);
  const installs = useInstalls(onRefresh);
  const restart = useRestart();
  const [dockerTest, setDockerTest] = useState<SelfTestResult>("idle");
  const [vmTest, setVmTest] = useState<SelfTestResult>("idle");
  // The option the player picked on the engine / hypervisor steps (null = not picked yet).
  const [engine, setEngine] = useState<DockerEngine | null>(null);
  const [hypervisor, pickHypervisor] = useState<string | null>(null);
  // The hypervisor picked here is the one labs run on from now on (the header and the lab page
  // follow), not only the one this flow sets up.
  const setHypervisor = (p: Provider | null) => {
    pickHypervisor(p);
    setVmProvider(p);
  };
  return {
    ...installs,
    ...restart,
    dockerTest,
    setDockerTest,
    vmTest,
    setVmTest,
    onRefresh,
    engine,
    setEngine,
    hypervisor,
    setHypervisor,
    /** Picks the hypervisor this flow sets up, without making it the one labs run on (a row's Fix). */
    pickHypervisor,
    /** Something is running: the flow should not move to another step. */
    busy: installs.installing !== null || dockerTest === "running" || vmTest === "running",
  };
}
export type MachineSetupState = ReturnType<typeof useMachineSetup>;
