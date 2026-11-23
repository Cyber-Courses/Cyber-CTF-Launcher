import {
  labLaunch,
  labPlacement,
  labPark,
  labProvision,
  labResume,
  labStatus,
  labStop,
  type ActiveOperation,
  type DockerEngine,
  type LabPlacement,
  type LabPlacementChoice,
  type LabStatus,
  type Park,
  type Provider,
  type Runtime,
} from "@/lib/tauri";
import type { PortMode } from "@/lib/settings";
import { notify } from "@/lib/notify";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import { getLabState } from "@/features/labs/lab-store";
import { setLastRun } from "@/features/labs/runs/last-run";
import { appendDeployLog, beginDeploy, failDeploy, getDeploySnapshot, restoreDeploy } from "@/features/labs/runs/deploy-runs";
import { noteSignedOut } from "@/features/account/signed-out";
import { describeError, errorCode, isSignedOut } from "@/lib/errors";
import { tell } from "@/lib/failure";
import { translate, type MessageKey } from "@/lib/i18n";
import { failureTitleKey } from "@/features/labs/failure-hints";
import { askOfflineStart } from "@/features/labs/offline-start-prompt";
import { confirmNodeKeys } from "@/features/servers/node-keys-prompt";

// The operations on a lab (launch, stop, park, resume, provision) that useLabActions runs: each
// one's body, streamed into the lab's deploy log, and the run around it.

/** What an operation does, its output streamed through `log`. */
export type OperationBody = (log: (line: string) => void, runtime: LabRuntimeInfo) => Promise<void>;

/** What `runOperation` resolves to when the player called the operation off when asked: nothing
 *  ran, nothing failed, and nothing chained to it (Reset's start after its stop) runs either. */
export const CALLED_OFF = Symbol("called off");

/** An operation the player called off when asked (`nodeKeysConfirmed`): no failure, nothing to show. */
class CalledOff extends Error {}

/** Before an operation through saved server `host`'s node (a Proxmox server whose lab bridge is
 *  internal to it), whose SSH host keys the run checks against the confirmed ones: never
 *  confirmed yet (a server set up before), the player confirms them first, in the setup's own
 *  question. Throws `CalledOff` when the player declines; rejects, in the host key words, when the
 *  node offers other keys than the confirmed ones. */
async function nodeKeysConfirmed(host: string | null | undefined) {
  if (host != null && !(await confirmNodeKeys(host))) throw new CalledOff();
}

/** The saved server lab `lab` runs on, as its status says (read now when this window has none
 *  yet); null when it runs here or on a cloud account, or its status can't be read (the
 *  operation then says what is wrong). */
async function serverOf(lab: Lab, rt: LabRuntimeInfo): Promise<string | null> {
  const known: Partial<Record<string, LabStatus>> = getLabState().statuses;
  const status = known[lab.id] ?? (await labStatus(lab.id, rt.runtime).catch(() => null));
  return status?.place === "server" ? status.hostId : null;
}

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
 * a failure as the log's last (✗) line. Resolves to the failure, null once it went through, or
 * `CALLED_OFF` when the player declined a question it needed answered, so a caller chaining
 * operations (Reset = stop, then start) stops at a failed or called-off step. `settle` ends the
 * run once the lab's status after it is read.
 */
export async function runOperation(
  lab: Lab,
  op: ActiveOperation["op"],
  body: OperationBody,
  settle: (lab: Lab) => Promise<void>,
): Promise<string | null | typeof CALLED_OFF> {
  if (!lab.runtime) return null;
  const previous = getDeploySnapshot().runs[lab.id];
  beginDeploy(lab.id, op);
  try {
    await body((line) => {
      appendDeployLog(lab.id, line);
    }, lab.runtime);
    return null;
  } catch (e) {
    // The player said no to a question the operation needed answered: nothing ran, nothing
    // failed. The lab stays as it was (a stop called off leaves it running, not failed).
    if (e instanceof CalledOff) {
      restoreDeploy(lab.id, previous);
      return CALLED_OFF;
    }
    // The backend's answer to an operation started while another one runs on the lab (the
    // launcher's own, or the runner service's on the lab's run): refused, not failed.
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

/** Where lab `lab` runs for `choice`, by the backend's one rule (`labPlacement`): what its spec
 *  allows and what is ready here, then the choice, else the player's saved preference. */
const placementFor = (lab: Lab, rt: LabRuntimeInfo, choice: LabPlacementChoice) =>
  labPlacement(lab.id, rt.runtime, rt.repository && rt.commit ? { repository: rt.repository, commit: rt.commit } : null, choice);

/** Where a lab (declared `runtime`) starts with nothing picked (the rule's answer for an empty
 *  choice), for the Start dialog's first row; null when it starts nowhere yet (no hypervisor
 *  ready for a VM lab, say): the dialog then offers what there is. */
export const askStartPlacement = (id: string, runtime: Runtime, repository: string, commit: string): Promise<LabPlacement | null> =>
  labPlacement(id, runtime, { repository, commit }, NOTHING_PICKED).catch(() => null);

/** A choice with nothing picked: the saved preference decides. */
const NOTHING_PICKED: LabPlacementChoice = { here: false, host: null, provider: null, perMachineVm: false, engine: null, defaultPorts: null };

/** `last`, where a lab (declared `runtime`) was last started, as the choice it was: the player's
 *  own earlier one, every part of it picked. */
function choiceOfPlacement(last: LabPlacement, runtime: Runtime): LabPlacementChoice {
  switch (last.kind) {
    case "docker":
      return { ...NOTHING_PICKED, here: true, engine: last.engine, defaultPorts: last.defaultPorts };
    case "vms":
      // A container lab's VMs here are its one-VM-per-machine form.
      return { ...NOTHING_PICKED, here: true, provider: last.provider, perMachineVm: runtime === "DOCKER" };
    case "server":
      return { ...NOTHING_PICKED, host: last.host };
    case "cloud":
      return { ...NOTHING_PICKED, host: last.account, perMachineVm: last.perMachine };
  }
}

/** A lab started again with nothing newly picked, where it was `last` started: sent as the choice,
 *  so the rule still checks the lab runs there and it is ready, refusing as for any choice. */
export const launchAgain =
  (lab: Lab, last: LabPlacement): OperationBody =>
  async (log, rt) => {
    await launchAt(lab, await placementFor(lab, rt, choiceOfPlacement(last, rt.runtime)), log);
  };

/** Starts lab `lab` where `placement` says, as it is: the node's keys confirmed first on a
 *  server, the start offered offline when Cyber CTF can't be reached, then the ready notice. */
async function launchAt(lab: Lab, placement: LabPlacement, log: (line: string) => void) {
  if (placement.kind === "server") await nodeKeysConfirmed(placement.host);
  await launchOrOfferOffline(lab.title, (offline) => labLaunch({ labId: lab.id, placement, offline }, log));
  log(translate("labs.actions.labRunning"));
  setLastRun(lab.id, placement);
  void notify(translate("labs.actions.labReady"), translate(readyKey(placement), { title: lab.title }));
}

/** A cloud lab, created in the cloud account `host`. No attack box: the player works from
 *  their own machine. Without an account the backend refuses it, saying so (`pickCloudAccount`). */
export const cloudLaunch =
  (lab: Lab, host: string | null | undefined): OperationBody =>
  async (log, rt) => {
    await launchAt(lab, await placementFor(lab, rt, { ...NOTHING_PICKED, host: host ?? null }), log);
  };

/** How a container lab runs beyond plain containers: as one VM per machine (`perMachine`) — its
 *  VM recipes, as a VM lab runs — here on hypervisor `provider`, or on a cloud account (cloud-vm,
 *  no local `provider`). The launcher never runs its containers inside a VM. */
export type LocalVmPick = { provider?: Provider; perMachine?: boolean };

/** What the player picked for a (non-cloud) lab: the server or cloud account `host` (null: this
 *  machine; omitted: nothing picked, the saved preference decides); the hypervisor `vmProvider`
 *  here; a container lab as one VM per machine (`perMachineVm`); a container lab's host ports
 *  (`ports`) and engine (`engine`, null: the current context) here. */
export type LocalWhere = {
  host?: string | null;
  vmProvider?: Provider;
  ports: PortMode | null;
  perMachineVm?: boolean;
  /** The container engine a lab here runs on (null: the current context). */
  engine?: DockerEngine | null;
};

/** `where` as the backend's rule reads a choice: what is unpicked is left to the saved preference. */
function choiceOf(where: LocalWhere): LabPlacementChoice {
  return {
    here: where.host === null,
    host: where.host ?? null,
    provider: where.vmProvider ?? null,
    perMachineVm: where.perMachineVm ?? false,
    engine: where.engine ?? null,
    defaultPorts: where.ports === null ? null : where.ports === "default",
  };
}

/** A VM or container lab, started where `where` says (the rule fills what it leaves unpicked). */
export const localLaunch =
  (lab: Lab, where: LocalWhere): OperationBody =>
  async (log, rt) => {
    await launchAt(lab, await placementFor(lab, rt, choiceOf(where)), log);
  };

/** The ready notification's text for a lab started at `placement`. */
function readyKey(placement: LabPlacement): MessageKey {
  if (placement.kind === "cloud") return "labs.actions.readyInCloud";
  if (placement.kind === "server") return "labs.actions.readyOnServer";
  return "labs.actions.readyHere";
}

/** Stop and remove a lab wherever it runs. */
export const stopping =
  (lab: Lab): OperationBody =>
  async (log, rt) => {
    await nodeKeysConfirmed(await serverOf(lab, rt));
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
    await nodeKeysConfirmed(await serverOf(lab, rt));
    await labResume(lab.id, rt.runtime, log);
    log(translate("labs.actions.labRunning"));
    setLastRun(lab.id);
    void notify(translate("labs.actions.labReady"), translate("labs.actions.back", { title: lab.title }));
  };

/** Re-run a VM lab's provisioning on one machine (or all), streamed into its deploy log. On a
 *  Proxmox server reached through its node, the node's keys are confirmed first, as for a stop. */
export const provisioning =
  (lab: Lab, machine: string | null): OperationBody =>
  async (log, rt) => {
    await nodeKeysConfirmed(await serverOf(lab, rt));
    await labProvision(lab.id, rt.runtime, machine, log);
    log(machine ? translate("labs.actions.machineProvisioned", { machine }) : translate("labs.actions.labProvisioned"));
  };
