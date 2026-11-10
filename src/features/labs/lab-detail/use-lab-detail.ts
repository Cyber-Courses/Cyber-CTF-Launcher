"use client";

import { useState } from "react";
import type { AttackBox } from "@/features/labs/use-attack-box";
import { useLabCheck } from "@/features/labs/lab-health";
import type { RunTarget } from "@/features/labs/run-target";
import { useAttackBox } from "@/features/labs/use-attack-box";
import type { Lab } from "@/features/labs/use-labs";
import { useCloudPreview } from "@/features/labs/cloud-panel";
import { removing, restartTarget } from "@/features/labs/lab-detail/lab-state";
import { labRuntime } from "@/features/labs/lab-runtime";
import { placeOf } from "@/features/labs/place";
import { useObjectives } from "@/features/labs/objectives/use-objectives";
import { useLabPageState } from "@/features/labs/lab-detail/use-lab-page-state";
import { useLabShell } from "@/features/labs/lab-detail/use-lab-shell";
import { useLabTools } from "@/features/labs/lab-detail/use-lab-tools";
import { usePageActions } from "@/features/labs/lab-detail/use-page-actions";
import { useRunTarget } from "@/features/labs/lab-detail/use-run-target";
import type { DeployRun } from "@/features/labs/runs/deploy-runs";
import { type DockerEngine, type LabStatus, type Park, type Provider, type Runtime } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

export type LabDetailProps = {
  lab: Lab;
  status?: LabStatus;
  /** This session's run on the lab (a start, a stop…): whether it is in flight, its log, what it
   *  is, when it began and how it failed. None before one. */
  deploy?: DeployRun;
  loggedIn: boolean;
  /** Logged out: the start button logs in instead of being greyed out. */
  onLogin?: () => Promise<boolean>;
  hostArch: string;
  /** Local hypervisors ready on this machine (Vagrant + hypervisor), for "in a VM". */
  readyVms?: Provider[];
  /** Whether a Docker engine answers on this machine (null = unknown). */
  dockerRunning?: boolean | null;
  /** The container engine the CLI uses here, and every engine that answers (for the Containers
   *  row's name and its engine switch). */
  dockerEngine?: DockerEngine | null;
  dockerEnginesRunning?: DockerEngine[];
  onBack: () => void;
  onStart: (target: RunTarget) => Promise<void>;
  /** Resolves to the failure (a string) when the stop failed: Reset then doesn't start the lab again. */
  onStop: () => Promise<unknown>;
  /** Pause (state saved) or shut down (powered off), machines kept for `onResume`. */
  onPark?: (mode: Park) => Promise<unknown>;
  onResume?: () => Promise<unknown>;
  /** Re-run a VM lab's provisioners on one machine (null = all), in place. */
  onProvision?: (machine: string | null) => Promise<unknown>;
  /** False until the first checks (sign-in, the lab's status) are in: the page then shows a
   *  placeholder instead of a Start button that flips to Running a moment later. */
  ready?: boolean;
};

/** No run on the lab in this session. */
const NO_DEPLOY: DeployRun = { busy: false, logs: [], times: [] };

type PageState = ReturnType<typeof useLabPageState>;

/** The attack box: a machine the launcher adds to the lab wherever it runs (here, on a server or
 *  in a cloud account), at the seat the lab declares for the player: a container among a container
 *  lab's, a Kali VM among a VM lab's (a container lab run as one VM per machine included).
 *  Where the launcher reaches no run to put one in (an ESXi host, a cloud-services lab, whose
 *  player works from their own machine), its status says so (`reachable`).
 *  Not while the lab itself is starting, resuming or stopping: the box goes in by updating the
 *  lab's run, which must not race the lab's own operation.
 *  A failed launch or resume can leave the machines up (a provisioning step broke): the backend
 *  then reports the lab as not running, so no box starts beside a lab that isn't ready, here as
 *  in the auto-start with each lab. */
function useLabAttackBox(labId: string, s: PageState, runtime: Runtime) {
  return useAttackBox(labId, { running: s.running, holding: s.deploying, runtime });
}

/** `value`, held at what it was when `hold` began, for as long as it lasts. */
function useHeld<T>(value: T, hold: boolean): T {
  const [held, setHeld] = useState(value);
  if (!hold && held !== value) setHeld(value);
  return hold ? held : value;
}

/** The attack box as the page shows it: while the lab is being removed, as it was before (the
 *  box's status is dropped for the operation), so its panel and the diagram's attacker stay
 *  put, dimmed, instead of flipping to "not started" and back should the removal fail. */
function useShownBox(box: AttackBox, hold: boolean): AttackBox {
  const status = useHeld(box.status, hold);
  return hold && status !== box.status ? { ...box, status } : box;
}

/** Reset = stop and start again where it ran: a clean lab. */
function useLabReset(onStop: LabDetailProps["onStop"], onStart: LabDetailProps["onStart"], target: () => RunTarget, after: () => void) {
  const t = useT();
  const [resetting, setResetting] = useState(false);
  async function reset() {
    setResetting(true);
    try {
      // A stop that failed left the lab half removed: starting over it would build on the wreck.
      const failure = await onStop();
      if (typeof failure === "string") {
        tell(t("labs.health.resetStopFailed"))(failure);
        return;
      }
      await onStart(target());
    } finally {
      setResetting(false);
      after();
    }
  }
  return { resetting, reset };
}

/** Everything the lab page reads: its derived state, where it starts, its actions in flight, the
 *  attack box, the health check, the observers, the objectives and Reset. */
export function useLabDetail(p: LabDetailProps) {
  const t = useT();
  const { lab, status, readyVms = [], deploy = NO_DEPLOY } = p;
  const rt = lab.runtime;
  const { acting, act, provisionTarget, setProvisionTarget } = usePageActions(p);
  const s = useLabPageState(lab.id, {
    status,
    runtime: rt,
    hostArch: p.hostArch,
    readyVms,
    busy: deploy.busy,
    acting,
    parkable: !!p.onPark,
    provisionable: !!p.onProvision,
    logs: deploy.logs,
    times: deploy.times,
    runOp: deploy.op,
    runStartedAt: deploy.startedAt,
    runFailureCode: deploy.failureCode,
    runFailureHolder: deploy.failureHolder,
  });
  const run = useRunTarget(lab, readyVms, p.dockerEngine ?? null, p.dockerEnginesRunning ?? []);
  const preview = useCloudPreview(lab.id, rt);
  // The lab's catalogue runtime: the backend resolves the form it runs in (`local_runtime`).
  const runtime = labRuntime(rt);
  const shell = useLabShell(lab.id, lab.title, runtime);
  const box = useShownBox(useLabAttackBox(lab.id, s, runtime), removing(s));
  const [check, clearCheck] = useLabCheck(lab.id, runtime, { running: s.running, downCount: s.down.length });
  const tools = useLabTools(lab.id, s.running && !s.isCloud, runtime);
  // An imported lab's challenges are its own `.ctf/objectives.json`'s, read with the lab: none from the backend.
  const objectives = useObjectives(lab.id, lab.question, t("labs.detail.objective"), p.loggedIn, !lab.imported);
  const restart = () => restartTarget(status, run.hosts, runtime, run.runOn, run.localVm);
  const { resetting, reset } = useLabReset(p.onStop, p.onStart, restart, clearCheck);
  const startOn = (target: RunTarget) => void p.onStart(target);
  // Where it runs, said the same way by the header, Details and the diagram's host card.
  const place = placeOf(status, s.isDocker, p.dockerEngine ?? null, run.hosts);
  return {
    s,
    busy: deploy.busy,
    acting,
    act,
    provisionTarget,
    setProvisionTarget,
    run,
    preview,
    shell,
    box,
    check,
    tools,
    objectives,
    resetting,
    reset,
    startOn,
    place,
  };
}

export type LabPage = ReturnType<typeof useLabDetail>;
