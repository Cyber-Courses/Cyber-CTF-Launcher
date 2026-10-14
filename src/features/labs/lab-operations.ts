import { serverList, labLaunch, labPark, labProvision, labResume, labStop, type ActiveOperation, type Park, type Provider } from "@/lib/tauri";
import { getAttackBox, getAttackImage, getAutoAttackBox, getVmProvider, type PortMode } from "@/lib/settings";
import { notify } from "@/lib/notify";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import { localProviders, pickLocalHypervisor, readyHypervisors, runsNatively } from "@/features/labs/lab-runtime";
import { setLastRun } from "@/lib/last-run";
import { appendDeployLog, beginDeploy, failDeploy, getDeploySnapshot, noteSignedOut, restoreDeploy } from "@/lib/deploy-store";
import { describeError, errorCode, isSignedOut } from "@/lib/errors";
import { noteStopped } from "@/features/labs/lab-store";
import { tell } from "@/lib/failure";
import { translate } from "@/lib/i18n";
import { failureTitleKey } from "@/features/labs/failure-hints";
import { askOfflineStart } from "@/features/labs/offline-start-prompt";

// The operations on a lab (launch, stop, park, resume, provision) that useLabActions runs: each
// one's body, streamed into the lab's deploy log, and the run around it.

/** What an operation does, its output streamed through `log`. */
export type OperationBody = (log: (line: string) => void, runtime: LabRuntimeInfo) => Promise<void>;

/** The machine report, to pick a hypervisor that is actually installed here. */
export type LaunchReport = {
  arch?: string;
  vagrant: { installed: boolean };
  vmProviders: { provider: Provider; remote: boolean; available: boolean; hypervisor?: boolean | null }[];
} | null;

/** Runs `launch`; when it fails because Cyber CTF can't be reached and the lab, installed here,
 *  can start without it (the `offlineReady` code, labs/offline.rs), asks the player (the lab
 *  `title`) and, on yes, runs it again offline. */
export async function launchOrOfferOffline(title: string, launch: (offline: boolean) => Promise<void>) {
  try {
    await launch(false);
  } catch (e) {
    if (errorCode(e) !== "offlineReady" || !(await askOfflineStart(title))) throw e;
    await launch(true);
  }
}

/**
 * One operation on a lab: a run in the deploy store, its output streamed into the run's log,
 * a failure as the log's last (✗) line. Resolves to the failure, or null once it went through,
 * so a caller chaining operations (Reset = stop, then start) can stop at a failed step.
 * `settle` ends the run once the lab's status after it is read.
 */
export async function runOperation(lab: Lab, op: ActiveOperation["op"], body: OperationBody, settle: (lab: Lab) => Promise<void>): Promise<string | null> {
  if (!lab.runtime) return null;
  const previous = getDeploySnapshot().runs[lab.id];
  let refused = false;
  beginDeploy(lab.id, op);
  try {
    await body((line) => appendDeployLog(lab.id, line), lab.runtime);
    return null;
  } catch (e) {
    // The backend's answer to an operation started while another one runs on the lab
    // (deploy_worker::start_worker): refused, not failed.
    refused = errorCode(e) === "alreadyInProgress";
    if (refused) {
      // The backend refused it (another operation on the lab is in flight): nothing ran, so
      // nothing failed. Keep the run that was there and say why instead of a ✗ in its log,
      // which read as a failed deploy.
      restoreDeploy(lab.id, previous);
      tell(translate("labs.actions.refused"))(e);
    } else {
      failDeploy(lab.id, e);
      // The account's session is gone (a cloud launch needs it): the shell reads it again.
      if (isSignedOut(e)) noteSignedOut();
      // Said wherever the player is (a toast, or a notification when the window is away), not
      // only in the lab's own deployment panel: a failed removal otherwise went unnoticed.
      const title = failureTitleKey(op);
      if (title) tell(translate(title, { title: lab.title }))(e);
    }
    return describeError(e);
  } finally {
    // Down now, by this app's hand: a scan answer from before this can't bring it back.
    if (!refused && (op === "stop" || op === "pause" || op === "shutdown")) noteStopped(lab.id);
    await settle(lab);
  }
}

/** A cloud lab, created in the cloud account `host`. No attack box: the player works from
 *  their own machine. */
export const cloudLaunch =
  (lab: Lab, host: string | null | undefined): OperationBody =>
  async (log) => {
    if (!host) throw new Error(translate("labs.actions.pickCloudAccount"));
    await labLaunch(lab.id, null, host, null, false, log);
    log(translate("labs.actions.labRunning"));
    setLastRun(lab.id);
    void notify(translate("labs.actions.labReady"), translate("labs.actions.readyInCloud", { title: lab.title }));
  };

/** The hypervisor a lab starts on here (a VM lab, or a container lab in a local VM), null for
 *  one on a server or in Docker here. Throws when a VM lab can't run here. */
function localHypervisor(rt: LabRuntimeInfo, remote: boolean, vmProvider: Provider | undefined, report: LaunchReport | undefined): Provider | null {
  const vm = rt.runtime === "VM";
  const preferred = getVmProvider();
  // Hypervisors ready on this machine (Vagrant + the tool), the Settings default first;
  // unknown (undefined) until the machine report has loaded.
  const ready = report ? readyHypervisors(report, preferred) : undefined;
  // A VM lab built for another CPU (x86 Windows on Apple Silicon) runs here only emulated,
  // on QEMU.
  const local = localProviders(rt, report?.arch);
  const foreign = vm && !!report?.arch && !runsNatively(rt, report.arch);
  const inLocalVm = !vm && !remote && !!vmProvider;
  const provider = inLocalVm ? vmProvider! : vm && !remote ? pickLocalHypervisor(local, ready, preferred) : null;
  // x86 VMs (Windows AD labs) boot on an Apple Silicon Mac only under QEMU's emulation; say
  // so up front instead of failing deep in Vagrant.
  if (!remote && foreign && provider === null) {
    throw new Error(translate("labs.actions.foreignCpu", { archs: rt.architectures.join(", "), arch: report!.arch ?? "" }));
  }
  if (vm && !remote && provider === null) {
    throw new Error(translate("labs.actions.noHypervisor", { providers: local.join(", ") || translate("labs.actions.none") }));
  }
  return provider;
}

/** A VM or container lab: on the server `host` (omitted: a VM lab's default server, when it
 *  supports its hypervisor), here in Docker or on a local hypervisor, or in a VM here on
 *  `vmProvider`. `ports` = the host ports of a container lab here. */
export const localLaunch =
  (lab: Lab, host: string | null | undefined, vmProvider: Provider | undefined, report: LaunchReport | undefined, ports: PortMode | null): OperationBody =>
  async (log, rt) => {
    const vm = rt.runtime === "VM";
    if (vm && host === undefined) host = await defaultHostFor(lab);
    // Docker labs go to a server host only when one is picked explicitly.
    const remote = host != null;
    const inLocalVm = !vm && !remote && !!vmProvider;
    const provider = localHypervisor(rt, remote, vmProvider, report);
    // The lab network isn't reachable from here, so an attack box goes next to the lab.
    // Remotely or inside a local VM: the container attack box image. A VM lab here: the
    // attack VM's Vagrant box, when Settings start the attack box with each lab.
    const attackbox = remote || inLocalVm ? getAttackImage() : vm && getAutoAttackBox() ? getAttackBox() : null;
    await launchOrOfferOffline(lab.title, (offline) => labLaunch(lab.id, provider, remote ? host! : null, attackbox, ports === "default", log, offline));
    log(translate("labs.actions.labRunning"));
    setLastRun(lab.id);
    void notify(
      translate("labs.actions.labReady"),
      translate(remote ? "labs.actions.readyOnServer" : inLocalVm ? "labs.actions.readyInVm" : "labs.actions.readyHere", { title: lab.title }),
    );
  };

export const stopping =
  (lab: Lab): OperationBody =>
  async (log, rt) => {
    await labStop(lab.id, rt.runtime, log);
    log(translate("labs.actions.labStopped"));
  };

/** Pause (state saved) or shut down (powered off) a lab, keeping its machines for a resume. */
export const parking =
  (lab: Lab, mode: Park): OperationBody =>
  async (log, rt) => {
    await labPark(lab.id, rt.runtime, mode, log);
    log(translate(mode === "pause" ? "labs.actions.labPaused" : "labs.actions.labShutDown"));
  };

/** Bring a parked lab back as it was: no rebuild, no new launch. */
export const resuming =
  (lab: Lab): OperationBody =>
  async (log, rt) => {
    await labResume(lab.id, rt.runtime, log);
    log(translate("labs.actions.labRunning"));
    setLastRun(lab.id);
    void notify(translate("labs.actions.labReady"), translate("labs.actions.back", { title: lab.title }));
  };

/** Re-run a VM lab's provisioning on one machine (or all), streamed into its deploy log. */
export const provisioning =
  (lab: Lab, machine: string | null): OperationBody =>
  async (log, rt) => {
    await labProvision(lab.id, rt.runtime, machine, log);
    log(machine ? translate("labs.actions.machineProvisioned", { machine }) : translate("labs.actions.labProvisioned"));
  };

/** The default server host id, if one is set and the lab supports its hypervisor. */
async function defaultHostFor(lab: Lab): Promise<string | null> {
  try {
    const { default: id, hosts } = await serverList();
    const host = hosts.find((h) => h.id === id);
    // Proxmox can't run labs yet (no working Vagrant provider), so it's never an implicit target.
    return host && host.provider !== "proxmox" && lab.runtime?.providers.includes(host.provider) ? host.id : null;
  } catch {
    return null;
  }
}
