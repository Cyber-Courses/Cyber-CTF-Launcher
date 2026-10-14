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

export function useMachineSetup(report: SystemReport | null, onRefresh: () => void) {
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

  const [installing, setInstalling] = useState<string | null>(null);
  const [installerOpened, setInstallerOpened] = useState(false);
  // "Turn on WSL 2" finished here: Windows needs a restart, whatever the last check said.
  const [wslTurnedOn, setWslTurnedOn] = useState(false);
  // Windows counts down to the restart that finishes WSL 2: kept here, not in the step, so a
  // remounted step (the onboarding draws each step afresh) can't ask for it a second time.
  const [restarting, setRestarting] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  // What the log is about (an install id or a plugin): each step shows only its own.
  const [logsFor, setLogsFor] = useState<string | null>(null);
  const [dockerTest, setDockerTest] = useState<SelfTestResult>("idle");
  const [vmTest, setVmTest] = useState<SelfTestResult>("idle");
  // The option the player picked on the engine / hypervisor steps (null = not picked yet).
  const [engine, setEngine] = useState<DockerEngine | null>(null);
  const [hypervisor, setHypervisor] = useState<string | null>(null);
  async function install(id: string, dep: Dependency, first: string) {
    setInstalling(id);
    setLogsFor(id);
    setLogs([first]);
    try {
      await installDependency(dep, (line) => setLogs((l) => [...l, line]));
      if (dep === "docker") setInstallerOpened(true);
      if (dep === "wsl") setWslTurnedOn(true);
    } catch (e) {
      setLogs((l) => [...l, `✗ ${describeError(e)}`]);
    } finally {
      setInstalling(null);
      onRefresh();
    }
  }

  /** Restarts Windows to finish turning WSL 2 on; `reason` is the note Windows shows. */
  async function restart(reason: string) {
    if (restarting) return;
    setRestarting(true);
    try {
      await machineRestart(reason);
    } catch (e) {
      setRestarting(false);
      tell(translate("machine.body.wsl.restartFailed"))(e);
    }
  }

  /** Adds the Vagrant plugin a hypervisor needs (e.g. vagrant-vmware-desktop). */
  async function installPlugin(plugin: string) {
    setInstalling(plugin);
    setLogsFor(plugin);
    setLogs([translate("machine.vm.pluginLog", { plugin })]);
    try {
      await installVagrantPlugin(plugin, (line) => setLogs((l) => [...l, line]));
    } catch (e) {
      setLogs((l) => [...l, `✗ ${describeError(e)}`]);
    } finally {
      setInstalling(null);
      onRefresh();
    }
  }

  return {
    installing,
    installerOpened,
    wslTurnedOn,
    restarting,
    restart,
    installPlugin,
    logs,
    logsFor,
    dockerTest,
    setDockerTest,
    vmTest,
    setVmTest,
    install,
    onRefresh,
    engine,
    setEngine,
    hypervisor,
    setHypervisor,
    /** Something is running: the flow should not move to another step. */
    busy: installing !== null || dockerTest === "running" || vmTest === "running",
  };
}
export type MachineSetupState = ReturnType<typeof useMachineSetup>;
