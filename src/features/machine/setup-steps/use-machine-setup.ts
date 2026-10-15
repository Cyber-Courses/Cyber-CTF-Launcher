import { useEffect, useRef, useState } from "react";
import { type SelfTestResult } from "@/features/machine/self-test";
import { getVmProvider } from "@/lib/settings";
import {
  installDependency,
  installVagrantPlugin,
  machineRestart,
  machineSelftestPrefetch,
  type Dependency,
  type DockerEngine,
  type SystemReport,
} from "@/lib/tauri";
import { hasHypervisor, isDockerReady } from "@/features/machine/setup-steps/steps";
import { ignore, tell } from "@/lib/failure";
import { translate } from "@/lib/i18n";
import { describeError } from "@/lib/errors";

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
  const [logs, setLogs] = useState<string[]>([]);
  // What the log is about (an install id or a plugin): each step shows only its own.
  const [logsFor, setLogsFor] = useState<string | null>(null);

  /** Runs install `id`, its log starting at `first`; a failure ends the log with "✗ <why>". */
  async function run(id: string, first: string, task: (onLog: (line: string) => void) => Promise<void>) {
    setInstalling(id);
    setLogsFor(id);
    setLogs([first]);
    const append = (line: string) => {
      setLogs((l) => [...l, line]);
    };
    await task(append).catch((e: unknown) => {
      append(`✗ ${describeError(e)}`);
    });
    setInstalling(null);
    onRefresh();
  }

  const install = (id: string, dep: Dependency, first: string) => {
    void run(id, first, async (onLog) => {
      await installDependency(dep, onLog);
      if (dep === "docker") setInstallerOpened(true);
      if (dep === "wsl") setWslTurnedOn(true);
    });
  };
  /** Adds the Vagrant plugin a hypervisor needs (e.g. vagrant-vmware-desktop). */
  const installPlugin = (plugin: string) => {
    void run(plugin, translate("machine.vm.pluginLog", { plugin }), (onLog) => installVagrantPlugin(plugin, onLog));
  };
  return { installing, installerOpened, wslTurnedOn, logs, logsFor, install, installPlugin };
}

/** Windows counts down to the restart that finishes WSL 2: kept with the flow, not the step, so a
 *  remounted step (the onboarding draws each step afresh) can't ask for it a second time. */
function useRestart() {
  const [restarting, setRestarting] = useState(false);
  /** Restarts Windows to finish turning WSL 2 on; `reason` is the note Windows shows. */
  const restart = (reason: string) => {
    if (restarting) return;
    setRestarting(true);
    machineRestart(reason).catch((e: unknown) => {
      setRestarting(false);
      tell(translate("machine.body.wsl.restartFailed"))(e);
    });
  };
  return { restarting, restart };
}

/** Install and test state for the setup steps. One per flow, passed to every step body. */
export function useMachineSetup(report: SystemReport | null, onRefresh: () => void) {
  usePrefetch(report);
  const installs = useInstalls(onRefresh);
  const restart = useRestart();
  const [dockerTest, setDockerTest] = useState<SelfTestResult>("idle");
  const [vmTest, setVmTest] = useState<SelfTestResult>("idle");
  // The option the player picked on the engine / hypervisor steps (null = not picked yet).
  const [engine, setEngine] = useState<DockerEngine | null>(null);
  const [hypervisor, setHypervisor] = useState<string | null>(null);
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
    /** Something is running: the flow should not move to another step. */
    busy: installs.installing !== null || dockerTest === "running" || vmTest === "running",
  };
}
export type MachineSetupState = ReturnType<typeof useMachineSetup>;
