"use client";

import { useCallback } from "react";
import { type ActiveOperation, type Park, type Provider } from "@/lib/tauri";
import { getPortMode, type PortMode } from "@/lib/settings";
import { askPortMode } from "@/features/labs/port-mode-prompt";
import type { Lab } from "@/features/labs/use-labs";
import { endDeploy, useDeploy, type DeployRun } from "@/features/labs/runs/deploy-runs";
import {
  cloudLaunch,
  localLaunch,
  parking,
  provisioning,
  resuming,
  runOperation,
  stopping,
  type LaunchReport,
  type OperationBody,
} from "@/features/labs/lab-operations";

/**
 * Start/stop actions for labs, shared across screens. The busy lab, its streamed log lines
 * and which lab they belong to live in a module store (runs/deploy-runs), so a deploy started
 * here keeps streaming and stays visible even after the labs screen unmounts (e.g. the user
 * opens Settings) and comes back. Keyed per lab, so two deploys at once keep separate logs; the
 * UI reads each lab's run from `runs[labId]`. The operations themselves are in lab-operations.
 */
export function useLabActions(refresh: (lab: Lab, opts?: { fresh?: boolean }) => Promise<void> | void) {
  // By lab id: none for a lab not run since the app started.
  const { runs }: { runs: Partial<Record<string, DeployRun>> } = useDeploy();

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
  const operate = useCallback((lab: Lab, op: ActiveOperation["op"], body: OperationBody) => runOperation(lab, op, body, settle), [settle]);

  /**
   * `host` = a server host id to run a VM lab on, null for this machine. Omitted, VM labs
   * go to the default server host when the lab supports its hypervisor. `vmProvider` runs a
   * container lab in a VM on this machine (its deploy/vagrant lab host) on that hypervisor.
   * A cloud lab runs only on the cloud account `host` (never one picked implicitly: it costs).
   * `report` (the machine report) picks a hypervisor that is actually installed here;
   * `chosenPorts` = the host ports for a container lab here, already chosen ("Where should it run?").
   */
  const launch = useCallback(
    async (lab: Lab, host?: string | null, vmProvider?: Provider, report?: LaunchReport, chosenPorts?: PortMode) => {
      if (!lab.runtime) return;
      if (lab.runtime.runtime === "CLOUD") {
        await operate(lab, "launch", cloudLaunch(lab, host));
        return;
      }
      // A container lab on this machine publishes its services here: on random free ports or
      // on the lab's own, the player's call (asked unless Settings remember it). Cancel = no start.
      const containersHere = lab.runtime.runtime !== "VM" && host == null && !vmProvider;
      const ports = containersHere ? (chosenPorts ?? getPortMode() ?? (await askPortMode(lab.title))) : null;
      if (containersHere && !ports) return;
      await operate(lab, "launch", localLaunch(lab, host, vmProvider, report, ports));
    },
    [operate],
  );

  const stop = useCallback((lab: Lab) => operate(lab, "stop", stopping(lab)), [operate]);
  /** Pause (state saved) or shut down (powered off) a lab, keeping its machines for `resume`. */
  const park = useCallback((lab: Lab, mode: Park) => operate(lab, mode, parking(lab, mode)), [operate]);
  /** Bring a parked lab back as it was: no rebuild, no new launch. */
  const resume = useCallback((lab: Lab) => operate(lab, "resume", resuming(lab)), [operate]);
  /** Re-run a VM lab's provisioning on one machine (or all), streamed into its deploy log. */
  const provision = useCallback((lab: Lab, machine: string | null) => operate(lab, "provision", provisioning(lab, machine)), [operate]);

  return { runs, launch, stop, park, resume, provision };
}
