"use client";

import { useState } from "react";
import type { AttackBox } from "@/features/labs/use-attack-box";
import { useLabCheck } from "@/features/labs/lab-health";
import type { RunTarget } from "@/features/labs/run-on";
import { useAttackBox } from "@/features/labs/use-attack-box";
import type { Lab } from "@/features/labs/use-labs";
import { useCloudPreview } from "@/features/labs/cloud-panel";
import { removing, restartTarget } from "@/features/labs/lab-detail/lab-state";
import { useObjectives } from "@/features/labs/objectives/use-objectives";
import { useHostedRun } from "@/features/labs/lab-detail/use-hosted-run";
import { useLabPageState } from "@/features/labs/lab-detail/use-lab-page-state";
import { useLabShell } from "@/features/labs/lab-detail/use-lab-shell";
import { useLabTools } from "@/features/labs/lab-detail/use-lab-tools";
import { usePageActions } from "@/features/labs/lab-detail/use-page-actions";
import { useRunTarget } from "@/features/labs/lab-detail/use-run-target";
import { type LabStatus, type Park, type Provider } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import type { ErrorCode } from "@/lib/errors";

export type LabDetailProps = {
  lab: Lab;
  status?: LabStatus;
  busy: boolean;
  logs: string[];
  times: number[];
  /** This session's last run on the lab: what it was (launch, stop…) and when it began. */
  runOp?: string;
  runStartedAt?: number;
  /** The code of the error this session's last run failed with. */
  runFailureCode?: ErrorCode | null;
  loggedIn: boolean;
  /** Logged out: the start button logs in instead of being greyed out. */
  onLogin?: () => Promise<void>;
  hostArch: string;
  /** Local hypervisors ready on this machine (Vagrant + hypervisor), for "in a VM". */
  readyVms?: Provider[];
  /** Whether a Docker engine answers on this machine (null = unknown). */
  dockerRunning?: boolean | null;
  onBack: () => void;
  onStart: (target: RunTarget) => Promise<void> | void;
  /** Resolves to the failure when the stop failed (Reset then doesn't start the lab again). */
  onStop: () => Promise<string | null | void> | void;
  /** Pause (state saved) or shut down (powered off), machines kept for `onResume`. */
  onPark?: (mode: Park) => Promise<unknown> | void;
  onResume?: () => Promise<unknown> | void;
  /** Re-run a VM lab's provisioners on one machine (null = all), in place. */
  onProvision?: (machine: string | null) => Promise<unknown> | void;
  /** False until the first checks (sign-in, the lab's status) are in: the page then shows a
   *  placeholder instead of a Start button that flips to Running a moment later. */
  ready?: boolean;
};

type PageState = ReturnType<typeof useLabPageState>;

/** The attack box: a container on a local container lab's networks, the learner's own VM beside
 *  a local VM lab; a remote container lab's runs next to it on its host.
 *  Not while the lab itself is starting, resuming or stopping: a resume brings the attack VM
 *  back on its own, and a second `vagrant up` in its folder at the same time would collide.
 *  A failed deploy can leave the VMs up (a provisioning step broke): the lab then reads as
 *  running, but its attack box must not start beside a lab that isn't ready.
 *  A cloud lab has none: the player works from their own machine. */
function useLabAttackBox(labId: string, s: PageState) {
  return useAttackBox(labId, {
    running: s.running && !s.isCloud,
    holding: s.deploying,
    local: !s.remote && !s.isCloud,
    kind: s.isDocker ? "container" : "vm",
    failed: s.deployFailed,
  });
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
  const { lab, status, readyVms = [] } = p;
  const rt = lab.runtime;
  const { acting, act, provisionTarget, setProvisionTarget } = usePageActions(p);
  const s = useLabPageState(lab.id, {
    status,
    runtime: rt,
    hostArch: p.hostArch,
    readyVms,
    busy: p.busy,
    acting,
    parkable: !!p.onPark,
    provisionable: !!p.onProvision,
    logs: p.logs,
    times: p.times,
    runOp: p.runOp,
    runStartedAt: p.runStartedAt,
    runFailureCode: p.runFailureCode,
  });
  const run = useRunTarget(rt, readyVms, p.hostArch);
  const preview = useCloudPreview(lab.id, rt);
  const hosted = useHostedRun(lab.id);
  const shell = useLabShell(lab.id, lab.title, s.remote || s.isDocker);
  const box = useShownBox(useLabAttackBox(lab.id, s), removing(s));
  const [check, clearCheck] = useLabCheck(lab.id, { running: s.running, downCount: s.down.length, enabled: s.isDocker });
  const tools = useLabTools(lab.id, s.running && !s.isCloud, s.isDocker);
  const objectives = useObjectives(lab.id, lab.question, t("labs.detail.objective"), p.loggedIn);
  const restart = () => restartTarget(status, run.hosts, run.runOn, run.localVm);
  const { resetting, reset } = useLabReset(p.onStop, p.onStart, restart, clearCheck);
  const startOn = (target: RunTarget) => (target.kind === "hosted" ? hosted.launch() : void p.onStart(target));
  return { s, acting, act, provisionTarget, setProvisionTarget, run, preview, hosted, shell, box, check, tools, objectives, resetting, reset, startOn };
}

export type LabPage = ReturnType<typeof useLabDetail>;
