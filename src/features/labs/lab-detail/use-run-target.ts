"use client";

import { useCallback, useState } from "react";
import { cloudAccountsFor, formProviders, hostSupports, isCloudLab, isContainerLab, vmFormTarget } from "@/features/labs/lab-runtime";
import type { PickerStep, Platform } from "@/features/labs/run-on";
import { fromSavedForm, isLocalVmForm, toSavedForm, type LocalVmForm, type RunTarget } from "@/features/labs/run-target";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import {
  getDockerEngine,
  getLocalVmForm,
  getPortMode,
  getVmProvider,
  setDockerEngine,
  setLocalVmForm,
  setPortMode,
  setVmProvider,
  type PortMode,
} from "@/lib/settings";
import { type DockerEngine, type LabRunOptions, type LabStartPlace, type Provider, type ServerHost } from "@/lib/tauri";
import { useLabAnswer, useLabRunOptions } from "@/features/labs/use-lab-answer";
import { askDefaultPlace } from "@/features/labs/lab-operations";
import { useSavedHosts } from "@/features/servers/use-server-list";

/** A container lab on cloud account `id`, when the lab's run options have it there in both
 *  forms: one VM (cloud-docker) and one VM per machine (cloud-vm), picked on the form step after
 *  the account. */
function cloudForms(options: LabRunOptions | null, hosts: ServerHost[], id: string): RunTarget[] {
  const h = hosts.find((x) => x.id === id);
  const on = (paths: LabRunOptions["cloudVm"]) => !!h && paths.some((p) => p.provider === h.provider);
  return options && on(options.cloudDocker) && on(options.cloudVm)
    ? [
        { kind: "host", id },
        { kind: "host", id, perMachine: true },
      ]
    : [];
}

/**
 * Where and how a lab starts, across the dialog's steps: the place ("Where should it run?"), then —
 * only when there is a choice — the form, its hypervisor, or the container engine and ports. Each
 * extra choice is its own step (full rows, no inline switches); a step with nothing to choose, or a
 * remembered choice, is skipped, and "don't ask again" on the last step remembers the whole path.
 */
export function useRunTarget(
  lab: { id: string; runtime: LabRuntimeInfo | null },
  readyVms: Provider[],
  dockerEngine: DockerEngine | null = null,
  dockerEnginesRunning: DockerEngine[] = [],
) {
  const rt = lab.runtime;
  const isCloud = isCloudLab(rt);
  const isDocker = isContainerLab(rt);
  // The container engines that answer here and the one a lab here runs on (see useEngineChoice).
  const { engine, engines, chosenEngine, setPickedEngine } = useEngineChoice(isDocker, dockerEngine, dockerEnginesRunning);
  // "Don't ask again": checked, Start saves the path's choices so next time skips the steps.
  const [remember, setRemember] = useState(false);
  // The player's own pick, kept over what Start points at first (late answers must not reset it).
  const [picked, setRunOn] = useState<RunTarget | null>(null);
  // A container lab here: its host ports (Settings' choice first).
  const [ports, setPorts] = useState<PortMode>(() => getPortMode() ?? "random");
  // Every way the lab starts, and where it starts with nothing picked: the backend's answers.
  const optionsRead = useLabRunOptions(lab.id, rt);
  const options = optionsRead.value;
  const place = useLabAnswer(lab.id, rt, askDefaultPlace).value;
  // The VM forms a container lab can run here as, and the hypervisors each can run on.
  const vm = useLocalVmChoice(rt, { readyVms, options }, isDocker, setRunOn);
  const { localVm, localVms, perMachineVm, perMachineVms, perMachine, pickProvider } = vm;
  const hostOk = useCallback((h: ServerHost) => hostSupports(rt, h, options), [rt, options]);
  const { hosts } = useSavedHosts();
  const cloudAccounts = cloudAccountsFor(rt, hosts, options);
  // A server picked, then removed in its setup window: back to where Start points at first.
  const runOn = picked && (picked.kind !== "host" || hosts.some((h) => h.id === picked.id)) ? picked : initialTarget(rt, hosts, place, options);
  const vmForms = vmFormTargets(isCloud, isDocker, localVm, perMachineVm);
  const cloudFormsFor = (id: string) => cloudForms(options, hosts, id);
  const pickedHost = runOn.kind === "host" ? hosts.find((h) => h.id === runOn.id) : undefined;
  // What the form step offers: the cloud account's forms when one is picked, else this machine's.
  const formTargets = runOn.kind === "host" ? cloudFormsFor(runOn.id) : vmForms;
  const hypervisorsFor = (form: RunTarget): Provider[] => (form.kind === "local-vm" ? localVms : form.kind === "local-vms" ? perMachineVms : []);
  const steps = usePickerSteps({ runOn, setRunOn, setPickedEngine, pickProvider, vmForms, cloudFormsFor, hypervisorsFor, engines });
  // Start, with "don't ask again" checked, saves what the path chose so the steps are skipped next time.
  const commitChoice = () => {
    if (remember) saveLaunchChoice(runOn, { ports, engine, engineCount: engines.length, hypervisorCount: hypervisorsFor(runOn).length });
  };

  return {
    hosts,
    hostOk,
    /** The lab's run options (each place's launch path and badge); null while they load, or when
     *  they couldn't be read. */
    runOptions: options,
    /** The run options as read: still being read, or why they couldn't be. */
    runOptionsRead: optionsRead,
    runOn,
    setRunOn,
    /** The step flow: where the dialog is, what each step picks, Back (see usePickerSteps). */
    steps,
    ports,
    setPorts,
    /** The helper-VM hypervisor a container lab here would use (for restarting it in the same one). */
    localVm,
    /** The VM forms This machine's "In a VM" row offers (helper VM, one VM per machine). */
    vmForms,
    /** The form step's rows: a picked cloud account's forms (one VM / one VM per machine), else
     *  This machine's VM forms. Empty when there is no form to choose. */
    formTargets,
    /** The cloud account the form step's rows belong to (for their logo and badge), when the
     *  form step shows a cloud account's forms rather than this machine's. */
    formAccount: pickedHost,
    /** The hypervisor rows' default (marked, preselected): the Settings choice, else the first ready. */
    defaultHypervisor: hypervisorsFor(runOn).at(0) ?? null,
    /** The container engines: the one a lab here runs on (`current`), every engine that answers
     *  (`all`, the engine step's rows when more than one), what the launch records (`chosen`, null
     *  = the current context), and the row marked and preselected (`preselected`: the current
     *  context, else the first). */
    engines: { current: engine, all: engines, chosen: chosenEngine, preselected: runningOf(dockerEngine, engines) ?? engines.at(0) ?? null },
    /** "Don't ask again" on the last step: checked, Start saves the path (`commit`). */
    dontAskAgain: { checked: remember, set: setRemember, commit: commitChoice },
    /** The per-machine VM form here (null: not this lab, or no ready hypervisor can run it). */
    perMachine,
    /** A cloud lab: the player's accounts it can run on, and the one picked. */
    cloudAccounts,
    pickedAccount: runOn.kind === "host" ? cloudAccounts.find((h) => h.id === runOn.id) : undefined,
    /** With servers saved, a VM here, several container engines not yet settled, or a cloud lab,
     *  Start asks first; otherwise it runs here at once (carrying any saved engine). */
    hasChoice: isCloud || hosts.length > 0 || localVm !== null || perMachine !== null || (engines.length > 1 && getDockerEngine() === null),
    /** The target picked, with the host ports and chosen engine for a container lab here. */
    chosen: (): RunTarget => (runOn.kind === "local" && isDocker ? { kind: "local", ports, engine: chosenEngine ?? undefined } : runOn),
  };
}

/** `e` when it is one of the container engines that answer here, else null. */
function runningOf(e: DockerEngine | null, engines: DockerEngine[]): DockerEngine | null {
  return e && engines.includes(e) ? e : null;
}

/** The saved hypervisor (Settings' choice) when it is one of `hyps`, else null: its step is then
 *  skipped straight to it ("don't ask again"). */
function savedHyp(hyps: Provider[]): Provider | null {
  const p = getVmProvider();
  return p && hyps.includes(p) ? p : null;
}

/** The saved VM form's target when it is still available in `vmForms`, else null: its step is then
 *  skipped straight to it. */
function savedFormTarget(vmForms: RunTarget[]): RunTarget | null {
  const saved = getLocalVmForm();
  return saved ? (vmForms.find((t) => t.kind === fromSavedForm(saved)) ?? null) : null;
}

/** The container engines that answer here and the one a lab runs on: the engine-step pick, else the
 *  saved choice, else the current context, else the first. `chosenEngine` is what the launch records
 *  (null = follow the current context). Empty for a VM/cloud lab. */
function useEngineChoice(isDocker: boolean, current: DockerEngine | null, running: DockerEngine[]) {
  const engines = isDocker ? running : [];
  const [picked, setPicked] = useState<DockerEngine | null>(null);
  const saved = isDocker ? getDockerEngine() : null;
  const chosenEngine = runningOf(picked, engines) ?? runningOf(saved, engines) ?? null;
  const engine = chosenEngine ?? runningOf(current, engines) ?? engines.at(0) ?? null;
  return { engine, engines, chosenEngine, setPickedEngine: setPicked };
}

/** What the path chose, and how many engines and hypervisors it had to choose from. */
type LaunchChoice = { ports: PortMode; engine: DockerEngine | null; engineCount: number; hypervisorCount: number };

/** Saves the path's choices for next time: the engine (when there was one to choose) and the ports
 *  for the container path; the VM form and its hypervisor (when there was one to choose, as
 *  Settings' choice) for the VM path. Nothing else saves them: browsing the steps changes nothing. */
function saveLaunchChoice(runOn: RunTarget, { ports, engine, engineCount, hypervisorCount }: LaunchChoice) {
  if (runOn.kind === "local") {
    setPortMode(ports);
    if (engineCount > 1) setDockerEngine(engine);
  } else if (isLocalVmForm(runOn)) {
    setLocalVmForm(toSavedForm(runOn.kind));
    if (hypervisorCount > 1) setVmProvider(runOn.provider);
  }
}

/** This machine's VM forms for a container lab: the helper VM (`localVm`, on any ready hypervisor in
 *  `localVms`) and, for a dual-form lab, one VM per machine (`perMachine`, on `perMachineVms`, which
 *  excludes UTM). `pickProvider` selects the form and its hypervisor for this launch only (Start
 *  saves it with "don't ask again"). Both default to the hypervisor chosen in Settings (readyVms lists it first), else the first ready. */
function useLocalVmChoice(
  rt: LabRuntimeInfo | null,
  { readyVms, options }: { readyVms: Provider[]; options: LabRunOptions | null },
  isDocker: boolean,
  setRunOn: (t: RunTarget) => void,
) {
  // The helper VM is this machine's own CPU's; one VM per machine runs the lab's own, on an
  // emulator only when that is another (the backend's run options say which).
  const localVms = isDocker && rt && options ? readyVms.filter((p) => formProviders(options.dockerVm).includes(p)) : [];
  const perMachineVms = isDocker && rt && options ? readyVms.filter((p) => formProviders(options.vms).includes(p)) : [];
  const [switched, setSwitched] = useState<Provider | null>(null);
  const resolved = (list: Provider[]) => (switched && list.includes(switched) ? switched : list.at(0)) ?? null;
  const localVm = resolved(localVms);
  const perMachineVm = resolved(perMachineVms);
  const perMachine = perMachineVm ? { provider: perMachineVm, providers: perMachineVms } : null;
  const pickProvider = (form: LocalVmForm["kind"], provider: Provider) => {
    setSwitched(provider);
    setRunOn(vmFormTarget(form, provider));
  };
  return { localVm, localVms, perMachineVm, perMachineVms, perMachine, pickProvider };
}

/** The VM forms This machine's "In a VM" row offers: a VM lab runs its own VMs (`local`); a
 *  container lab runs a helper VM (`local-vm`) and, when dual-form, one VM per machine (`local-vms`). */
function vmFormTargets(isCloud: boolean, isDocker: boolean, localVm: Provider | null, perMachineVm: Provider | null): RunTarget[] {
  if (isCloud) return [];
  if (!isDocker) return [{ kind: "local" }];
  const forms: RunTarget[] = [];
  if (localVm) forms.push({ kind: "local-vm", provider: localVm });
  if (perMachineVm) forms.push({ kind: "local-vms", provider: perMachineVm });
  return forms;
}

type StepsDeps = {
  runOn: RunTarget;
  setRunOn: (t: RunTarget) => void;
  setPickedEngine: (e: DockerEngine) => void;
  pickProvider: (form: LocalVmForm["kind"], provider: Provider) => void;
  vmForms: RunTarget[];
  cloudFormsFor: (id: string) => RunTarget[];
  hypervisorsFor: (form: RunTarget) => Provider[];
  engines: DockerEngine[];
};

/**
 * The dialog's steps. Step 1 picks a place (`pickPlatform`). The container path then asks for the
 * engine (`pickEngine`, only with more than one) and the host ports; the VM path asks for the form
 * (`pickForm`, only with two) and its hypervisor (`pickHypervisor`, only with more than one). A step
 * with nothing to choose, or a remembered choice, is skipped. `isLastStep` marks the step that
 * finishes the path (where "don't ask again" sits); `back` walks one step back.
 */
function usePickerSteps(d: StepsDeps) {
  const [step, setStep] = useState<PickerStep>("platform");
  const { runOn, vmForms, hypervisorsFor, engines } = d;
  const savedEngine = () => runningOf(getDockerEngine(), engines);
  const savedPorts = () => getPortMode() !== null;
  const hasEngineStep = engines.length > 1 && !savedEngine();
  const hasFormStep = vmForms.length >= 2 && !savedFormTarget(vmForms);
  const curHyps = hypervisorsFor(runOn);
  // Whether this step finishes the path (no further choice): the engine step when the ports are
  // remembered, the form step when the chosen form needs no hypervisor choice, else ports/hypervisor.
  const isLastStep =
    step === "ports" || step === "hypervisor" || (step === "engine" && savedPorts()) || (step === "form" && (curHyps.length <= 1 || !!savedHyp(curHyps)));

  const pickForm = (form: RunTarget) => {
    // A cloud account's form (one VM / one VM per machine) is the last step: set it and let Start
    // finish, no hypervisor to pick.
    if (form.kind === "host") {
      d.setRunOn(form);
      return;
    }
    const formHyps = hypervisorsFor(form);
    const saved = savedHyp(formHyps);
    if (isLocalVmForm(form)) d.pickProvider(form.kind, saved ?? form.provider);
    else d.setRunOn(form);
    // More than one hypervisor (not remembered) → its step; else this form is settled (stay on the
    // form step, Start finishes it, or the platform step when the form step was skipped).
    setStep(formHyps.length > 1 && !saved ? "hypervisor" : hasFormStep ? "form" : "platform");
  };
  const pickPlatform = (p: Platform) => {
    if (p.kind === "host") {
      d.setRunOn({ kind: "host", id: p.id });
      // A cloud account offering both forms opens the form step ("How should it run?"); otherwise
      // the account is settled and Start runs there.
      setStep(d.cloudFormsFor(p.id).length >= 2 ? "form" : "platform");
      return;
    }
    if (p.kind === "containers") {
      d.setRunOn({ kind: "local" });
      setStep(hasEngineStep ? "engine" : savedPorts() ? "platform" : "ports");
      return;
    }
    // "In a VM": two forms (not remembered) open the form step; one goes straight to its form.
    if (hasFormStep) {
      d.setRunOn(vmForms[0]);
      setStep("form");
    } else {
      pickForm(savedFormTarget(vmForms) ?? vmForms.at(0) ?? { kind: "local" });
    }
  };
  const pickEngine = (e: DockerEngine) => {
    d.setPickedEngine(e);
    setStep(savedPorts() ? "engine" : "ports");
  };
  const pickHypervisor = (provider: Provider) => {
    if (isLocalVmForm(runOn)) d.pickProvider(runOn.kind, provider);
  };
  const back = () => {
    if (step === "ports") setStep(hasEngineStep ? "engine" : "platform");
    else if (step === "hypervisor") setStep(hasFormStep ? "form" : "platform");
    else setStep("platform");
  };
  const resetSteps = () => {
    setStep("platform");
  };
  return { step, isLastStep, hypervisors: curHyps, pickPlatform, pickForm, pickHypervisor, pickEngine, back, resetSteps };
}

/** Where Start points at first: a cloud lab's first account it runs on; else where the backend
 *  starts the lab with nothing picked (`place`: where it last ran, a ready hypervisor here, the
 *  default server), in the form and on the hypervisor it says; here until it answers. */
export function initialTarget(rt: LabRuntimeInfo | null, hosts: ServerHost[], place: LabStartPlace | null, options: LabRunOptions | null = null): RunTarget {
  if (isCloudLab(rt)) {
    const account = cloudAccountsFor(rt, hosts, options).at(0);
    return account ? { kind: "host", id: account.id } : { kind: "local" };
  }
  if (place?.host) return hosts.some((h) => h.id === place.host) ? { kind: "host", id: place.host } : { kind: "local" };
  if (!place?.provider) return { kind: "local" };
  return isContainerLab(rt) ? vmFormTarget(place.perMachineVm ? "local-vms" : "local-vm", place.provider) : { kind: "local", provider: place.provider };
}

export type RunTargetChoice = ReturnType<typeof useRunTarget>;
