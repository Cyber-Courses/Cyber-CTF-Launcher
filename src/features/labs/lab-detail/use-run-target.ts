"use client";

import { useCallback, useEffect, useState } from "react";
import { cloudAccountsFor, hostSupports, isCloudLab, localProviders, perMachineProviders } from "@/features/labs/lab-runtime";
import type { PickerStep, Platform, RunTarget } from "@/features/labs/run-on";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { getPortMode, setVmProvider, type PortMode } from "@/lib/settings";
import { SERVER_CHANGED, serverList, type DockerEngine, type Provider, type ServerHost } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { useLatest } from "@/lib/use-latest";

/**
 * Where a lab starts, picked in "Where should it run?": this machine or one of the player's
 * server hosts (Docker labs through their deploy/ layer), or a VM here for a container lab.
 * VM labs default to the default host; Docker labs to here; a cloud lab to the first of the
 * player's accounts on a cloud it runs on (never a default host: a server).
 */
export function useRunTarget(rt: LabRuntimeInfo | null, readyVms: Provider[], hostArch: string, dockerEngine: DockerEngine | null = null, dockerEnginesRunning: DockerEngine[] = []) {
  const isCloud = isCloudLab(rt);
  const isDocker = rt?.runtime !== "VM" && !isCloud;
  // The container engines that answer here (several can run side by side) and the one a lab here
  // will use — the current context when it answers, else the first. Only for a container lab.
  const { engine, engines } = resolveEngines(isDocker, dockerEngine, dockerEnginesRunning);
  const [{ hosts, defaultId }, setServers] = useState<{ hosts: ServerHost[]; defaultId: string | null }>(NO_SERVERS);
  // The player's own pick, kept over what Start points at first (a server list read late, or
  // the machine report arriving, must not reset it).
  const [picked, setRunOn] = useState<RunTarget | null>(null);
  // A container lab here: its host ports (Settings' choice first).
  const [ports, setPorts] = useState<PortMode>(() => getPortMode() ?? "random");
  // A container lab can also run in a VM here: one helper VM (`localVm`) and, for a dual-form lab,
  // one VM per machine (`perMachine`), on the hypervisor chosen in Settings, else the first ready.
  const { localVm, localVms, pickVm, perMachineVm, perMachine } = useLocalVmChoice(rt, readyVms, hostArch, isDocker, setRunOn);
  const hostOk = useCallback((h: ServerHost) => hostSupports(rt, h), [rt]);
  // Prefer this machine when a local hypervisor can run the lab; fall back to the default server
  // only when none can. A default server shouldn't silently capture every VM lab.
  const localReady = isDocker || (!!rt && readyVms.some((p) => localProviders(rt, hostArch).includes(p)));
  const [serversChanged, setServersChanged] = useState(0);
  // Only the newest read is kept: the one on mount can answer after one a change asked for.
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
  }, [begin, serversChanged]);
  // A server or cloud account added (or removed) in its setup window: the page follows at once,
  // instead of asking to "Connect an account" until it is left and opened again.
  useTauriEvent(SERVER_CHANGED, () => {
    setServersChanged((n) => n + 1);
  });
  const cloudAccounts = cloudAccountsFor(rt, hosts);
  // A server picked, then removed in its setup window: back to where Start points at first.
  const runOn = picked && (picked.kind !== "host" || hosts.some((h) => h.id === picked.id)) ? picked : initialTarget(rt, hosts, defaultId, localReady);
  // The picker's two steps and the moves between them, over the VM forms This machine offers.
  const steps = usePickerSteps(runOn, setRunOn, vmFormTargets(isCloud, isDocker, localVm, perMachineVm));

  return {
    hosts,
    hostOk,
    runOn,
    setRunOn,
    // `step`, `pickPlatform`, `back`, `resetSteps`: the picker's two-step flow (see usePickerSteps).
    ...steps,
    ports,
    setPorts,
    localVm,
    localVms,
    pickVm,
    /** The container engine a lab here will run on (null: none answers, or not a container lab),
     *  and every engine that answers, for the Containers row and (later) its switch. */
    engine,
    engines,
    /** The per-machine VM form here, for a lab with VM recipes too (null: not this lab, or no
     *  ready hypervisor can run it). */
    perMachine,
    /** A cloud lab: the player's accounts it can run on, and the one picked. */
    cloudAccounts,
    pickedAccount: runOn.kind === "host" ? cloudAccounts.find((h) => h.id === runOn.id) : undefined,
    /** With servers saved (or a VM here), Start asks where to run first; a cloud lab always asks
     *  (which account, and what it costs there). */
    hasChoice: isCloud || hosts.length > 0 || localVm !== null || perMachine !== null,
    /** The target picked, with the host ports for a container lab here. */
    chosen: (): RunTarget => (runOn.kind === "local" && isDocker ? { kind: "local", ports } : runOn),
  };
}

const NO_SERVERS = { hosts: [], defaultId: null };

/** The container engines that answer here and the one a lab will use (the current context when it
 *  answers, else the first). Empty for a VM or cloud lab. */
function resolveEngines(isDocker: boolean, current: DockerEngine | null, running: DockerEngine[]): { engine: DockerEngine | null; engines: DockerEngine[] } {
  const engines = isDocker ? running : [];
  const engine = (current && engines.includes(current) ? current : engines.at(0)) ?? null;
  return { engine, engines };
}

/** This machine's VM forms for a container lab: the helper VM (`localVm`, with every ready
 *  hypervisor in `localVms`) and, for a dual-form lab, one VM per machine (`perMachine`). Both run
 *  on the hypervisor chosen in Settings (readyVms lists it first), else the first ready; picking one
 *  (in either option) saves it as Settings' choice and selects that form. */
function useLocalVmChoice(rt: LabRuntimeInfo | null, readyVms: Provider[], hostArch: string, isDocker: boolean, setRunOn: (t: RunTarget) => void) {
  const localVms = isDocker && rt ? readyVms.filter((p) => localProviders(rt, hostArch).includes(p)) : [];
  const [switched, setSwitched] = useState<Provider | null>(null);
  const localVm = (switched && localVms.includes(switched) ? switched : localVms.at(0)) ?? null;
  const setLocal = (p: Provider) => {
    setVmProvider(p);
    setSwitched(p);
  };
  const pickVm = (p: Provider) => {
    setLocal(p);
    setRunOn({ kind: "local-vm", provider: p });
  };
  const perMachineVms = isDocker && rt ? readyVms.filter((p) => perMachineProviders(rt, hostArch).includes(p)) : [];
  const perMachineVm = (switched && perMachineVms.includes(switched) ? switched : perMachineVms.at(0)) ?? null;
  const pickPerMachineVm = (p: Provider) => {
    setLocal(p);
    setRunOn({ kind: "local-vms", provider: p });
  };
  const perMachine = perMachineVm ? { provider: perMachineVm, providers: perMachineVms, onPick: pickPerMachineVm } : null;
  return { localVm, localVms, pickVm, perMachineVm, perMachine };
}

/** The VM forms This machine's "In a VM" row offers: a VM lab runs its own VMs (`local`); a
 *  container lab runs a helper VM (`local-vm`) and, when dual-form, one VM per machine
 *  (`local-vms`). With two, picking "In a VM" opens step 2; with one it starts that form. */
function vmFormTargets(isCloud: boolean, isDocker: boolean, localVm: Provider | null, perMachineVm: Provider | null): RunTarget[] {
  if (isCloud) return [];
  if (!isDocker) return [{ kind: "local" }];
  const forms: RunTarget[] = [];
  if (localVm) forms.push({ kind: "local-vm", provider: localVm });
  if (perMachineVm) forms.push({ kind: "local-vms", provider: perMachineVm });
  return forms;
}

/** The picker's steps. Step 1 picks a place: a host or a lone VM form starts straight away; picking
 *  Containers opens the host-ports step; picking In a VM with two VM forms opens the VM-forms step.
 *  `resetSteps` (dialog open/close) and `back` (the footer) return to the places. */
function usePickerSteps(runOn: RunTarget, setRunOn: (t: RunTarget) => void, vmForms: RunTarget[]) {
  const [step, setStep] = useState<PickerStep>("platform");
  const toPlatform = () => {
    setStep("platform");
  };
  const pickPlatform = (p: Platform) => {
    if (p.kind === "host") {
      setRunOn({ kind: "host", id: p.id });
      setStep("platform");
      return;
    }
    if (p.kind === "containers") {
      // The host ports are their own step; the place is settled here.
      setRunOn({ kind: "local" });
      setStep("ports");
      return;
    }
    // "In a VM": two forms open the VM-forms step (keeping a VM form selected); one starts at once.
    if (vmForms.length >= 2) {
      if (runOn.kind !== "local-vm" && runOn.kind !== "local-vms") setRunOn(vmForms[0]);
      setStep("vmForms");
    } else {
      setRunOn(vmForms[0] ?? { kind: "local" });
      setStep("platform");
    }
  };
  return { step, pickPlatform, back: toPlatform, resetSteps: toPlatform };
}

/** Where Start points at first: a cloud lab's first account; the default host when this machine
 *  can't run the lab (`localReady`) and the host can; else here. */
export function initialTarget(rt: LabRuntimeInfo | null, hosts: ServerHost[], defaultId: string | null, localReady: boolean): RunTarget {
  if (isCloudLab(rt)) {
    const account = cloudAccountsFor(rt, hosts).at(0);
    return account ? { kind: "host", id: account.id } : { kind: "local" };
  }
  const def = hosts.find((h) => h.id === defaultId);
  return !localReady && def && hostSupports(rt, def) ? { kind: "host", id: def.id } : { kind: "local" };
}

export type RunTargetChoice = ReturnType<typeof useRunTarget>;
