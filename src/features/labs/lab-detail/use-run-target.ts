"use client";

import { useCallback, useEffect, useState } from "react";
import {
  cloudAccountsFor,
  cloudRunsPerMachine,
  hostSupports,
  isCloudLab,
  isContainerLab,
  localProviders,
  perMachineProviders,
} from "@/features/labs/lab-runtime";
import type { PickerStep, Platform, RunTarget } from "@/features/labs/run-on";
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
import { SERVER_CHANGED, labServers, serverList, type DockerEngine, type Provider, type ServerHost } from "@/lib/tauri";
import { ignore } from "@/lib/failure";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { useLatest } from "@/lib/use-latest";

/** A dual-form container lab on cloud account `id`, when it runs cloud-vm: two forms — one VM
 *  (cloud-docker), one VM per machine (cloud-vm) — picked on the form step after the account. */
function cloudForms(rt: LabRuntimeInfo | null, hosts: ServerHost[], id: string): RunTarget[] {
  const h = hosts.find((x) => x.id === id);
  return h && cloudRunsPerMachine(rt, h.provider)
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
  hostArch: string,
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
  // The player's own pick, kept over what Start points at first (a server list read late, or
  // the machine report arriving, must not reset it).
  const [picked, setRunOn] = useState<RunTarget | null>(null);
  // A container lab here: its host ports (Settings' choice first).
  const [ports, setPorts] = useState<PortMode>(() => getPortMode() ?? "random");
  // The VM forms a container lab can run here as, and the hypervisors each can run on.
  const { localVm, localVms, perMachineVm, perMachineVms, perMachine, pickProvider } = useLocalVmChoice(rt, readyVms, hostArch, isDocker, setRunOn);
  const servers = useLabServers(lab.id, rt);
  const hostOk = useCallback((h: ServerHost) => hostSupports(rt, h, servers), [rt, servers]);
  // Prefer this machine when a local hypervisor can run the lab; fall back to the default server
  // only when none can. A default server shouldn't silently capture every VM lab.
  const localReady = isDocker || (!!rt && readyVms.some((p) => localProviders(rt, hostArch).includes(p)));
  const { hosts, defaultId } = useServerList();
  const cloudAccounts = cloudAccountsFor(rt, hosts);
  // A server picked, then removed in its setup window: back to where Start points at first.
  const runOn = picked && (picked.kind !== "host" || hosts.some((h) => h.id === picked.id)) ? picked : initialTarget(rt, hosts, defaultId, localReady, servers);
  const vmForms = vmFormTargets(isCloud, isDocker, localVm, perMachineVm);
  const cloudFormsFor = (id: string) => cloudForms(rt, hosts, id);
  const pickedHost = runOn.kind === "host" ? hosts.find((h) => h.id === runOn.id) : undefined;
  // What the form step offers: the cloud account's forms when one is picked, else this machine's.
  const formTargets = runOn.kind === "host" ? cloudFormsFor(runOn.id) : vmForms;
  const hypervisorsFor = (form: RunTarget): Provider[] => (form.kind === "local-vm" ? localVms : form.kind === "local-vms" ? perMachineVms : []);
  const steps = usePickerSteps({ runOn, setRunOn, setPickedEngine, pickProvider, vmForms, cloudFormsFor, hypervisorsFor, engines });
  // Start, with "don't ask again" checked, saves what the path chose so the steps are skipped next time.
  const commitChoice = () => {
    if (remember) saveLaunchChoice(runOn, ports, engine, engines.length);
  };

  return {
    hosts,
    hostOk,
    runOn,
    setRunOn,
    // `step`, `pickPlatform`, `pickForm`, `pickHypervisor`, `pickEngine`, `back`, `resetSteps`,
    // `isLastStep`, `hypervisors`: the step flow (see usePickerSteps).
    ...steps,
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
    defaultHypervisor: runOn.kind === "local-vm" ? (localVms.at(0) ?? null) : runOn.kind === "local-vms" ? (perMachineVms.at(0) ?? null) : null,
    /** The container engine a lab here runs on, every engine that answers (engine step rows when
     *  more than one), and what the launch records (null = current context). */
    engine,
    engines,
    chosenEngine,
    /** The default engine row (marked, preselected): the current context, else the first. */
    defaultEngine: runningOf(dockerEngine, engines) ?? engines.at(0) ?? null,
    /** "Don't ask again" on the last step: checked, Start saves the path (`commitChoice`). */
    remember,
    setRemember,
    commitChoice,
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

/** The kinds of server (ESXi, Proxmox) lab `labId` runs on, by the backend's rule (its spec's
 *  targets, as for the default server); null until it answers, when it can't, or without the
 *  lab's source (the catalogue's providers alone decide then). */
function useLabServers(labId: string, rt: LabRuntimeInfo | null): Provider[] | null {
  const [answer, setAnswer] = useState<{ key: string; servers: Provider[] } | null>(null);
  const [runtime, repository, commit] = [rt?.runtime, rt?.repository, rt?.commit];
  const key = runtime && repository && commit ? `${labId}:${runtime}@${repository}/${commit}` : null;
  useEffect(() => {
    if (!key || !runtime || !repository || !commit) return;
    let live = true;
    labServers(labId, runtime, repository, commit)
      .then((servers) => {
        if (live) setAnswer({ key, servers });
      })
      .catch(ignore("without the lab's spec, its saved servers are offered by the catalogue alone"));
    return () => {
      live = false;
    };
  }, [labId, key, runtime, repository, commit]);
  return answer?.key === key ? answer.servers : null;
}

const NO_SERVERS: { hosts: ServerHost[]; defaultId: string | null } = { hosts: [], defaultId: null };

/** The player's saved servers and cloud accounts, kept fresh: a server or account added (or
 *  removed) in its setup window shows at once, and only the newest read is kept. */
function useServerList() {
  const [servers, setServers] = useState(NO_SERVERS);
  const [changed, setChanged] = useState(0);
  const begin = useLatest();
  useEffect(() => {
    const isLatest = begin();
    serverList()
      .then((l) => {
        if (isLatest()) setServers({ hosts: l.hosts, defaultId: l.default });
      })
      .catch(() => {
        if (isLatest()) setServers(NO_SERVERS);
      });
  }, [begin, changed]);
  useTauriEvent(SERVER_CHANGED, () => {
    setChanged((n) => n + 1);
  });
  return servers;
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
  const f = getLocalVmForm();
  return vmForms.find((t) => (f === "vm" && t.kind === "local-vm") || (f === "vms" && t.kind === "local-vms")) ?? null;
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

/** Saves the path's choices for next time: engine (when there was one to choose) and ports for the
 *  container path, else the VM form (its hypervisor is saved as it is picked). */
function saveLaunchChoice(runOn: RunTarget, ports: PortMode, engine: DockerEngine | null, engineCount: number) {
  if (runOn.kind === "local") {
    setPortMode(ports);
    if (engineCount > 1) setDockerEngine(engine);
  } else if (runOn.kind === "local-vm") {
    setLocalVmForm("vm");
  } else if (runOn.kind === "local-vms") {
    setLocalVmForm("vms");
  }
}

/** This machine's VM forms for a container lab: the helper VM (`localVm`, on any ready hypervisor in
 *  `localVms`) and, for a dual-form lab, one VM per machine (`perMachine`, on `perMachineVms`, which
 *  excludes UTM). `pickProvider` selects the form and its hypervisor, saving it as Settings' choice.
 *  Both default to the hypervisor chosen in Settings (readyVms lists it first), else the first ready. */
function useLocalVmChoice(rt: LabRuntimeInfo | null, readyVms: Provider[], hostArch: string, isDocker: boolean, setRunOn: (t: RunTarget) => void) {
  const localVms = isDocker && rt ? readyVms.filter((p) => localProviders(rt, hostArch).includes(p)) : [];
  const perMachineVms = isDocker && rt ? readyVms.filter((p) => perMachineProviders(rt, hostArch).includes(p)) : [];
  const [switched, setSwitched] = useState<Provider | null>(null);
  const resolved = (list: Provider[]) => (switched && list.includes(switched) ? switched : list.at(0)) ?? null;
  const localVm = resolved(localVms);
  const perMachineVm = resolved(perMachineVms);
  const perMachine = perMachineVm ? { provider: perMachineVm, providers: perMachineVms } : null;
  const pickProvider = (form: "local-vm" | "local-vms", provider: Provider) => {
    setVmProvider(provider);
    setSwitched(provider);
    setRunOn({ kind: form, provider });
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
  pickProvider: (form: "local-vm" | "local-vms", provider: Provider) => void;
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
  const curHyps = runOn.kind === "local-vm" || runOn.kind === "local-vms" ? hypervisorsFor(runOn) : [];
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
    const formHyps = form.kind === "local-vm" || form.kind === "local-vms" ? hypervisorsFor(form) : [];
    const saved = savedHyp(formHyps);
    const provider = saved ?? (form.kind === "local-vm" || form.kind === "local-vms" ? form.provider : null);
    if (provider && (form.kind === "local-vm" || form.kind === "local-vms")) d.pickProvider(form.kind, provider);
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
    if (runOn.kind === "local-vm" || runOn.kind === "local-vms") d.pickProvider(runOn.kind, provider);
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

/** Where Start points at first: a cloud lab's first account; the default host when this machine
 *  can't run the lab (`localReady`) and the host can (`servers`: the kinds of server it runs on,
 *  once known); else here. */
export function initialTarget(
  rt: LabRuntimeInfo | null,
  hosts: ServerHost[],
  defaultId: string | null,
  localReady: boolean,
  servers: readonly Provider[] | null = null,
): RunTarget {
  if (isCloudLab(rt)) {
    const account = cloudAccountsFor(rt, hosts).at(0);
    return account ? { kind: "host", id: account.id } : { kind: "local" };
  }
  const def = hosts.find((h) => h.id === defaultId);
  return !localReady && def && hostSupports(rt, def, servers) ? { kind: "host", id: def.id } : { kind: "local" };
}

export type RunTargetChoice = ReturnType<typeof useRunTarget>;
