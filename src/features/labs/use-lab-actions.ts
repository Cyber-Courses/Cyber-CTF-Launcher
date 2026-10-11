"use client";

import { useCallback } from "react";
import { serverList, labLaunch, labPark, labProvision, labResume, labStop, type ActiveOperation, type Park, type Provider } from "@/lib/tauri";
import { getAttackBox, getAttackImage, getAutoAttackBox, getPortMode, getVmProvider, type PortMode } from "@/lib/settings";
import { askPortMode } from "@/features/labs/port-mode-prompt";
import { notify } from "@/lib/notify";
import type { Lab, LabRuntimeInfo } from "@/features/labs/use-labs";
import { localProviders, pickLocalHypervisor, readyHypervisors, runsNatively } from "@/features/labs/lab-runtime";
import { setLastRun } from "@/lib/last-run";
import { appendDeployLog, beginDeploy, endDeploy, getDeploySnapshot, restoreDeploy, useDeploy } from "@/lib/deploy-store";
import { noteStopped } from "@/features/labs/lab-store";
import { tell } from "@/lib/failure";
import { translate } from "@/lib/i18n";

/**
 * Start/stop actions for labs, shared across screens. The busy lab, its streamed log lines
 * and which lab they belong to live in a module store (deploy-store), so a deploy started
 * here keeps streaming and stays visible even after the labs screen unmounts (e.g. the user
 * opens Settings) and comes back. Keyed per lab, so two deploys at once keep separate logs; the
 * UI reads each lab's run from `runs[labId]`.
 */
export function useLabActions(refresh: (lab: Lab, opts?: { fresh?: boolean }) => Promise<void> | void) {
  const { runs } = useDeploy();

  // An operation stays busy until the lab's status after it has been read: cleared earlier, the
  // page showed the old status's buttons (Shut down on a lab just shut down) for a few seconds,
  // and a click there started the same operation again. Capped, so a hung read never pins it.
  const settle = useCallback(
    async (lab: Lab) => {
      await Promise.race([Promise.resolve(refresh(lab, { fresh: true })), new Promise((r) => setTimeout(r, 20_000))]);
      endDeploy(lab.id);
    },
    [refresh],
  );

  // One operation on a lab: a run in the deploy store, its output streamed into the run's log,
  // a failure as the log's last (✗) line. Resolves to the failure, or null once it went through,
  // so a caller chaining operations (Reset = stop, then start) can stop at a failed step.
  const operate = useCallback(
    async (lab: Lab, op: ActiveOperation["op"], body: (log: (line: string) => void, runtime: LabRuntimeInfo) => Promise<void>): Promise<string | null> => {
      if (!lab.runtime) return null;
      const previous = getDeploySnapshot().runs[lab.id];
      let refused = false;
      beginDeploy(lab.id, op);
      try {
        await body((line) => appendDeployLog(lab.id, line), lab.runtime);
        return null;
      } catch (e) {
        refused = REFUSED.test(String(e));
        if (refused) {
          // The backend refused it (another operation on the lab is in flight): nothing ran, so
          // nothing failed. Keep the run that was there and say why instead of a ✗ in its log,
          // which read as a failed deploy.
          restoreDeploy(lab.id, previous);
          tell(translate("labs.actions.refused"))(e);
        } else {
          appendDeployLog(lab.id, `✗ ${String(e)}`);
        }
        return String(e);
      } finally {
        // Down now, by this app's hand: a scan answer from before this can't bring it back.
        if (!refused && (op === "stop" || op === "pause" || op === "shutdown")) noteStopped(lab.id);
        await settle(lab);
      }
    },
    [settle],
  );

  /** A cloud lab, created in the cloud account `host`. No attack box: the player works from
   *  their own machine. */
  const launchCloud = useCallback(
    (lab: Lab, host: string | null | undefined) =>
      operate(lab, "launch", async (log) => {
        if (!host) throw new Error(translate("labs.actions.pickCloudAccount"));
        await labLaunch(lab.id, null, host, null, false, log);
        log(translate("labs.actions.labRunning"));
        setLastRun(lab.id);
        notify(translate("labs.actions.labReady"), translate("labs.actions.readyInCloud", { title: lab.title }));
      }),
    [operate],
  );

  /**
   * `host` = a server host id to run a VM lab on, null for this machine. Omitted, VM labs
   * go to the default server host when the lab supports its hypervisor. `vmProvider` runs a
   * container lab in a VM on this machine (its deploy/vagrant lab host) on that hypervisor.
   * A cloud lab runs only on the cloud account `host` (never one picked implicitly: it costs).
   */
  const launch = useCallback(
    async (
      lab: Lab,
      host?: string | null,
      vmProvider?: Provider,
      /** The machine report, to pick a hypervisor that is actually installed here. */
      report?: {
        arch?: string;
        vagrant: { installed: boolean };
        vmProviders: { provider: Provider; remote: boolean; available: boolean; hypervisor?: boolean | null }[];
      } | null,
      /** Host ports for a container lab here, already chosen ("Where should it run?"). */
      chosenPorts?: PortMode,
    ) => {
      if (!lab.runtime) return;
      if (lab.runtime.runtime === "CLOUD") {
        await launchCloud(lab, host);
        return;
      }
      // A container lab on this machine publishes its services here: on random free ports or
      // on the lab's own, the player's call (asked unless Settings remember it). Cancel = no start.
      const containersHere = lab.runtime.runtime !== "VM" && host == null && !vmProvider;
      const ports = containersHere ? (chosenPorts ?? getPortMode() ?? (await askPortMode(lab.title))) : null;
      if (containersHere && !ports) return;
      await operate(lab, "launch", async (log, rt) => {
        const vm = rt.runtime === "VM";
        if (vm && host === undefined) host = await defaultHostFor(lab);
        // Docker labs go to a server host only when one is picked explicitly.
        const remote = host != null;
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
        // The lab network isn't reachable from here, so an attack box goes next to the lab.
        // Remotely or inside a local VM: the container attack box image. A VM lab here: the
        // attack VM's Vagrant box, when Settings start the attack box with each lab.
        const attackbox = remote || inLocalVm ? getAttackImage() : vm && getAutoAttackBox() ? getAttackBox() : null;
        await labLaunch(lab.id, provider, remote ? host! : null, attackbox, ports === "default", log);
        log(translate("labs.actions.labRunning"));
        setLastRun(lab.id);
        notify(
          translate("labs.actions.labReady"),
          translate(remote ? "labs.actions.readyOnServer" : inLocalVm ? "labs.actions.readyInVm" : "labs.actions.readyHere", { title: lab.title }),
        );
      });
    },
    [operate, launchCloud],
  );

  const stop = useCallback(
    (lab: Lab) =>
      operate(lab, "stop", async (log, rt) => {
        await labStop(lab.id, rt.runtime, log);
        log(translate("labs.actions.labStopped"));
      }),
    [operate],
  );

  /** Pause (state saved) or shut down (powered off) a lab, keeping its machines for `resume`. */
  const park = useCallback(
    (lab: Lab, mode: Park) =>
      operate(lab, mode, async (log, rt) => {
        await labPark(lab.id, rt.runtime, mode, log);
        log(translate(mode === "pause" ? "labs.actions.labPaused" : "labs.actions.labShutDown"));
      }),
    [operate],
  );

  /** Bring a parked lab back as it was: no rebuild, no new launch. */
  const resume = useCallback(
    (lab: Lab) =>
      operate(lab, "resume", async (log, rt) => {
        await labResume(lab.id, rt.runtime, log);
        log(translate("labs.actions.labRunning"));
        setLastRun(lab.id);
        notify(translate("labs.actions.labReady"), translate("labs.actions.back", { title: lab.title }));
      }),
    [operate],
  );

  /** Re-run a VM lab's provisioning on one machine (or all), streamed into its deploy log. */
  const provision = useCallback(
    (lab: Lab, machine: string | null) =>
      operate(lab, "provision", async (log, rt) => {
        await labProvision(lab.id, rt.runtime, machine, log);
        log(machine ? translate("labs.actions.machineProvisioned", { machine }) : translate("labs.actions.labProvisioned"));
      }),
    [operate],
  );

  return { runs, launch, stop, park, resume, provision };
}

/** The backend's answer to an operation started while another one runs on the lab
 *  (deploy_worker::run_job): refused, not failed. */
const REFUSED = /already in progress/i;

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
