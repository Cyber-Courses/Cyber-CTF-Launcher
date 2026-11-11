import {
  labDefaultPlace,
  labLaunch,
  labPark,
  labProvision,
  labResume,
  labStop,
  type ActiveOperation,
  type DockerEngine,
  type LabStartPlace,
  type Park,
  type Provider,
  type Runtime,
} from "@/lib/tauri";
import { getVmProvider, type PortMode } from "@/lib/settings";
import { notify } from "@/lib/notify";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import { setLastRun } from "@/features/labs/runs/last-run";
import { appendDeployLog, beginDeploy, failDeploy, getDeploySnapshot, restoreDeploy } from "@/features/labs/runs/deploy-runs";
import { noteSignedOut } from "@/features/account/signed-out";
import { describeError, errorCode, isSignedOut } from "@/lib/errors";
import { tell } from "@/lib/failure";
import { translate, type MessageKey } from "@/lib/i18n";
import { failureTitleKey } from "@/features/labs/failure-hints";
import { askOfflineStart } from "@/features/labs/offline-start-prompt";

// The operations on a lab (launch, stop, park, resume, provision) that useLabActions runs: each
// one's body, streamed into the lab's deploy log, and the run around it.

/** What an operation does, its output streamed through `log`. */
export type OperationBody = (log: (line: string) => void, runtime: LabRuntimeInfo) => Promise<void>;

/** Runs `launch`; when it fails because Cyber CTF can't be reached and the lab, installed here,
 *  can start without it (the `offlineReady` code, labs/offline.rs), asks the player (the lab
 *  `title`) and, on yes, runs it again offline. */
async function launchOrOfferOffline(title: string, launch: (offline: boolean) => Promise<void>) {
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
  beginDeploy(lab.id, op);
  try {
    await body((line) => {
      appendDeployLog(lab.id, line);
    }, lab.runtime);
    return null;
  } catch (e) {
    // The backend's answer to an operation started while another one runs on the lab
    // (deploy_worker::start_worker): refused, not failed.
    if (errorCode(e) === "alreadyInProgress") {
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
      tell(translate(failureTitleKey(op), { title: lab.title }))(e);
    }
    return describeError(e);
  } finally {
    await settle(lab);
  }
}

/** A cloud lab, created in the cloud account `host`. No attack box: the player works from
 *  their own machine. */
export const cloudLaunch =
  (lab: Lab, host: string | null | undefined): OperationBody =>
  async (log) => {
    // Without an account the backend refuses it, saying so (`pickCloudAccount`).
    await labLaunch({ labId: lab.id, provider: null, host: host ?? null, defaultPorts: false }, log);
    log(translate("labs.actions.labRunning"));
    setLastRun(lab.id);
    void notify(translate("labs.actions.labReady"), translate("labs.actions.readyInCloud", { title: lab.title }));
  };

/** The hypervisor a VM lab starts on here: the one picked (`picked`: where Reset starts it again,
 *  its status's), else the backend's (`place.hypervisor`: where it last ran here, the Settings
 *  choice, the first ready one that runs it). None when no hypervisor here runs it: the backend
 *  refuses the launch, saying so (`noHypervisor`). */
const vmHypervisor = (picked: Provider | undefined, place: LabStartPlace | null): Provider | null => picked ?? place?.hypervisor ?? null;

/** How a container lab runs beyond plain containers: in a VM here (`provider`, the hypervisor),
 *  and/or as one VM per machine (`perMachine`) — its VM recipes, as a VM lab runs, rather than its
 *  containers inside one helper VM. A cloud account's one-VM-per-machine form (cloud-vm) carries
 *  `perMachine` with no local `provider`. */
export type LocalVmPick = { provider?: Provider; perMachine?: boolean };

/** Where a (non-cloud) lab starts: on the server `host`; here in Docker or, a VM lab, on the
 *  hypervisor `vmProvider` (none: the backend's, `vmHypervisor`); in a VM here on `vmProvider`; or (`perMachineVm`, a container lab with VM recipes) as one VM
 *  per machine on `vmProvider`. `host` omitted (undefined): nothing was picked, and the backend
 *  says where (`defaultPlace`). `ports` = the host ports of a container lab here. */
export type LocalWhere = {
  host?: string | null;
  vmProvider?: Provider;
  ports: PortMode | null;
  perMachineVm?: boolean;
  /** The container engine a lab here runs on (null: the current context). */
  engine?: DockerEngine | null;
  /** `host` is a cloud account (its ready notification says so), not a server. */
  inCloud?: boolean;
};

/** Where a lab with no place picked starts, by the backend's one rule (`labDefaultPlace`: where
 *  it last ran, else a hypervisor ready here for a VM lab, the Settings choice first, else the
 *  default server the lab's spec runs on), read from its spec at the catalogue's commit. */
export const askDefaultPlace = (id: string, runtime: Runtime, repository: string, commit: string) =>
  labDefaultPlace(id, runtime, repository, commit, getVmProvider());

/** [`askDefaultPlace`] for a lab; without its source known, here (and no hypervisor). */
async function defaultPlace(id: string, rt: LabRuntimeInfo): Promise<LabStartPlace> {
  if (!rt.repository || !rt.commit) return { host: null, provider: null, perMachineVm: false, hypervisor: null };
  return askDefaultPlace(id, rt.runtime, rt.repository, rt.commit);
}

/** Where a lab starts, as picked (`where`) or, with nothing picked (Home's "start again", a Start
 *  with no choice), the backend's place in the form it says. A VM lab here asks it for its
 *  hypervisor too (`place`), unless one was picked. */
async function resolveWhere(lab: Lab, rt: LabRuntimeInfo, where: LocalWhere) {
  let { host, vmProvider, perMachineVm = false } = where;
  const isVmLab = rt.runtime === "VM";
  const place = host === undefined || (isVmLab && host === null && !vmProvider) ? await defaultPlace(lab.id, rt) : null;
  if (place && host === undefined) {
    host = place.host;
    if (place.provider) [vmProvider, perMachineVm] = [place.provider, !isVmLab && place.perMachineVm];
  }
  return { host, vmProvider, perMachineVm, place, isVmLab };
}

/** A VM or container lab, started where `where` says. */
export const localLaunch =
  (lab: Lab, where: LocalWhere): OperationBody =>
  async (log, rt) => {
    const { ports, engine } = where;
    const { host, vmProvider, perMachineVm, place, isVmLab } = await resolveWhere(lab, rt, where);
    // Run as one VM per machine, the lab is a VM lab here: its machines are VMs on `vmProvider`.
    const asVms = perMachineVm && !!vmProvider && host == null;
    // On a cloud account, one VM per machine is cloud-vm: no local hypervisor, the host carries it.
    const cloudPerMachine = perMachineVm && host != null;
    const vm = isVmLab || asVms;
    const remote = host != null;
    const inLocalVm = !vm && !remote && !!vmProvider;
    // Here: a VM lab on its hypervisor, a container lab's VM form on the one picked for it.
    const provider = remote ? null : isVmLab ? vmHypervisor(vmProvider, place) : (vmProvider ?? null);
    await launchOrOfferOffline(lab.title, (offline) =>
      labLaunch(
        {
          labId: lab.id,
          provider,
          host: host ?? null,
          defaultPorts: ports === "default",
          offline,
          perMachineVm: asVms || cloudPerMachine,
          // The chosen engine pins a plain container lab here; never for a VM form or a server.
          dockerEngine: vm || remote ? null : (engine ?? null),
        },
        log,
      ),
    );
    log(translate("labs.actions.labRunning"));
    setLastRun(lab.id);
    void notify(translate("labs.actions.labReady"), translate(readyKey(remote, where.inCloud ?? false, inLocalVm), { title: lab.title }));
  };

/** The ready notification's text for a lab started on a `remote` host (a cloud account when
 *  `inCloud`), else here, in a VM (`inLocalVm`) or not. */
function readyKey(remote: boolean, inCloud: boolean, inLocalVm: boolean): MessageKey {
  if (remote) return inCloud ? "labs.actions.readyInCloud" : "labs.actions.readyOnServer";
  return inLocalVm ? "labs.actions.readyInVm" : "labs.actions.readyHere";
}

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
