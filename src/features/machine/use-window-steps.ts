"use client";

import { useState } from "react";
import { firstPending, pathOf, pathSteps, type LastTests, type MachineStep, type SetupPath } from "@/features/machine/setup-steps";
import type { Provider, SystemReport } from "@/lib/tauri";
import { useStepper } from "@/lib/use-stepper";

/** A step of the setup window: the path chooser, a machine setup step, then the summary. */
export type WindowStep = "path" | MachineStep | "ready";

/** A request to open at a step (a "Fix" link from the Machine page); a new nonce jumps again. */
export type StartAt = { step: string; nonce: number } | null;

const stepsOf = (path: SetupPath | null, report: SystemReport): WindowStep[] => ["path", ...(path ? pathSteps(path, report) : []), "ready"];

/**
 * The window's steps and where the player is: the chooser, then the chosen path's steps (one lab
 * type's own, the others never shown), then the summary. A path started from the chooser opens
 * at its first step this machine has still to do (at the summary when none is left), so a
 * reopened setup never asks to redo what the report and the last tests say is done; `startAt`
 * (a "Fix" link) opens its own step, whatever its state.
 */
export function useWindowSteps(report: SystemReport, startAt: StartAt, last: LastTests, preferred: Provider | null) {
  const [path, setPath] = useState<SetupPath | null>(() => (startAt ? pathOf(startAt.step) : null));
  const steps = stepsOf(path, report);
  const { pos, next, back, goTo } = useStepper(steps.length);
  const [jumped, setJumped] = useState<number | null>(null);
  if (startAt && startAt.nonce !== jumped) {
    setJumped(startAt.nonce);
    const p = pathOf(startAt.step);
    if (p) {
      setPath(p);
      goTo(stepsOf(p, report).indexOf(startAt.step as MachineStep));
    }
  }
  const start = (p: SetupPath) => {
    setPath(p);
    const list = stepsOf(p, report);
    const pending = firstPending(p, report, last, preferred);
    goTo(pending ? list.indexOf(pending) : list.length - 1);
  };
  return { steps, path, at: pos, key: steps[pos], next, back, start };
}
