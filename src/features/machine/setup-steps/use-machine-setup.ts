import { useEffect, useRef, useState } from "react";
import { type SelfTestResult } from "@/features/machine/self-test";
import { getVmProvider, setVmProvider } from "@/lib/settings";
import type { Provider } from "@/lib/tauri";
import {
  installDependency,
  installVagrantPlugin,
  machineRestart,
  machineSelftestPrefetch,
  shareVirtualization,
  type Dependency,
  type DockerEngine,
  type InstallProgress,
  type Interaction,
  type SystemReport,
  type WindowsFeature,
} from "@/lib/tauri";
import { hasHypervisor, isDockerReady } from "@/features/machine/setup-steps/steps";
import { useRunLog } from "@/lib/use-run-log";
import { ignore, tell } from "@/lib/failure";
import { describeError } from "@/lib/errors";
import { translate } from "@/lib/i18n";
import { useToolInstallsOff } from "@/lib/use-managed";
import { asksOf, type PendingInstall } from "@/features/machine/install-asks";

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

/** An install waiting for the player to have read what it asks of them. */
type Asked = PendingInstall & { start: () => void };

/** What a flow's installs ask of the player: the install waiting for their consent (`pending`),
 *  and what the step running now asks (`asking`), which an install's progress sets (`progress`). */
function useAsking() {
  const [pending, setPending] = useState<Asked | null>(null);
  const [asking, setAsking] = useState<Interaction[] | null>(null);
  /** `onLog` as the sink of an install's progress: its lines into the log, what a step asks into
   *  the live hint. */
  const progress =
    (onLog: (line: string) => void) =>
    (p: InstallProgress): void => {
      if (p.kind === "line") onLog(p.line);
      else setAsking(p.interactions);
    };
  /** Drops the install waiting for the player: nothing runs. */
  const cancelPending = () => {
    setPending(null);
  };
  return { pending, setPending, cancelPending, asking, setAsking, progress };
}

/** The installs the steps run (a tool, a Vagrant plugin), one at a time, with their log, each
 *  followed by a new machine check. One that asks something of the player (a password prompt, the
 *  agreement a vendor's app will show them, a restart: the report's setup options say what) tells
 *  them first and waits for Install (`pending`), and what the step running asks shows while it
 *  runs (`asking`). Nothing accepts an agreement for them. */
function useInstalls(report: SystemReport | null, onRefresh: () => void) {
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
  // `pending` (the install waiting for the player), `cancelPending`, `asking` (what the step running asks).
  const { setPending, setAsking, progress, ...consent } = useAsking();

  /** Runs install `id`, its log starting at `first`; a failure ends the log with "✗ <why>" and
   *  is kept as the install's failure. */
  async function run(id: string, first: string, task: (onLog: (line: string) => void) => Promise<void>) {
    setInstalling(id);
    setLogsFor(id);
    setFailures((f) => ({ ...f, [id]: undefined }));
    setAsking(null);
    await runLog(first, async (onLog) => {
      try {
        await task(onLog);
      } catch (e) {
        setFailures((f) => ({ ...f, [id]: describeError(e) }));
        throw e;
      }
    });
    setInstalling(null);
    setAsking(null);
    onRefresh();
  }

  const installsOff = useToolInstallsOff();
  /** Installs `dep` under `id` (`name`: what the player calls it): at once when it asks nothing
   *  of them, else once they have read what it asks and pressed Install. */
  const install = (id: string, dep: Dependency, first: string, name: string) => {
    if (installsOff) return;
    const start = () => {
      setPending(null);
      void run(id, first, async (onLog) => {
        await installDependency(dep, progress(onLog));
        if (dep === "docker") setInstallerOpened(true);
        if (dep === "wsl") setWslTurnedOn(true);
      });
    };
    const asks = asksOf(report, dep);
    if (asks.length === 0) start();
    else setPending({ name, asks, start });
  };
  /** Adds the Vagrant plugin a hypervisor needs (e.g. vagrant-vmware-desktop). */
  const installPlugin = (plugin: string) => {
    if (installsOff) return;
    void run(plugin, translate("machine.vm.pluginLog", { plugin }), (onLog) => installVagrantPlugin(plugin, onLog));
  };
  /** Lets VirtualBox share the CPU's virtualization with KVM, under `id` (its hypervisor). */
  const share = (id: string) => {
    if (installsOff) return;
    void run(id, translate("machine.vm.shareLog"), (onLog) => shareVirtualization(progress(onLog)));
  };
  /** No install runs, and the policy allows one. */
  const canInstall = installing === null && !installsOff;
  /** Why install `id` failed in this flow, if it did (and wasn't run again since). */
  const failure = (id: string): string | null => failures[id] ?? null;
  return { installing, installsOff, canInstall, installerOpened, wslTurnedOn, logs: lines ?? [], logsFor, install, installPlugin, share, failure, ...consent };
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
  const installs = useInstalls(report, onRefresh);
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
    /** The installs the steps run, their log and their failures (`useInstalls`). */
    installs,
    /** The Windows restart that finishes WSL 2 or Hyper-V (`useRestart`). */
    restarts: restart,
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
/** The installs a setup flow runs (`MachineSetupState["installs"]`). */
export type SetupInstalls = MachineSetupState["installs"];
