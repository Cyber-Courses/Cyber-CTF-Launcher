"use client";

import { useCallback, useEffect, useState } from "react";
import { cloudAccountsFor, hostSupports, isCloudLab, localProviders, perMachineProviders } from "@/features/labs/lab-runtime";
import type { Platform, RunTarget } from "@/features/labs/run-on";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { getPortMode, setVmProvider, type PortMode } from "@/lib/settings";
import { SERVER_CHANGED, serverList, type Provider, type ServerHost } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { useLatest } from "@/lib/use-latest";

/**
 * Where a lab starts, picked in "Where should it run?": this machine or one of the player's
 * server hosts (Docker labs through their deploy/ layer), or a VM here for a container lab.
 * VM labs default to the default host; Docker labs to here; a cloud lab to the first of the
 * player's accounts on a cloud it runs on (never a default host: a server).
 */
export function useRunTarget(rt: LabRuntimeInfo | null, readyVms: Provider[], hostArch: string) {
  const isCloud = isCloudLab(rt);
  const isDocker = rt?.runtime !== "VM" && !isCloud;
  const [{ hosts, defaultId }, setServers] = useState<{ hosts: ServerHost[]; defaultId: string | null }>(NO_SERVERS);
  // The player's own pick, kept over what Start points at first (a server list read late, or
  // the machine report arriving, must not reset it).
  const [picked, setRunOn] = useState<RunTarget | null>(null);
  // A container lab here: its host ports (Settings' choice first).
  const [ports, setPorts] = useState<PortMode>(() => getPortMode() ?? "random");
  // A container lab can also run in a VM here: on the hypervisor chosen in Settings (readyVms
  // lists it first), else the first ready one its deploy/ supports. One option, not a catalogue;
  // with several ready, the option carries a switch, whose pick becomes Settings' choice.
  const localVms = isDocker && rt ? readyVms.filter((p) => localProviders(rt, hostArch).includes(p)) : [];
  const [switched, setSwitched] = useState<Provider | null>(null);
  const localVm = (switched && localVms.includes(switched) ? switched : localVms.at(0)) ?? null;
  const pickVm = (p: Provider) => {
    setVmProvider(p);
    setSwitched(p);
    setRunOn({ kind: "local-vm", provider: p });
  };
  // A container lab whose spec has VM recipes too can also run here as one VM per machine, like
  // a VM lab (Isoloom's vagrant target), on a ready hypervisor that form runs on (not UTM). The
  // same switch state: picking a hypervisor in either VM option is the one Settings' choice.
  const perMachineVms = isDocker && rt ? readyVms.filter((p) => perMachineProviders(rt, hostArch).includes(p)) : [];
  const perMachineVm = (switched && perMachineVms.includes(switched) ? switched : perMachineVms.at(0)) ?? null;
  const pickPerMachineVm = (p: Provider) => {
    setVmProvider(p);
    setSwitched(p);
    setRunOn({ kind: "local-vms", provider: p });
  };
  const perMachine = perMachineVm ? { provider: perMachineVm, providers: perMachineVms, onPick: pickPerMachineVm } : null;
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
  // The picker's two steps and the moves between them. This machine offers more than one form (so
  // a step 2) when a helper VM or a per-machine VM is ready; a VM lab or a hypervisor-less container
  // lab has one form and skips straight to Start.
  const steps = usePickerSteps(runOn, setRunOn, localVm !== null || perMachine !== null);

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

/** Every local form (`local`, `local-vm`, `local-vms`) belongs to the one "this machine" platform. */
const isLocalTarget = (t: RunTarget) => t.kind === "local" || t.kind === "local-vm" || t.kind === "local-vms";

/** The picker's two steps. Step 1 picks a platform: a host (one form today) or a single-form This
 *  machine selects it and stays on step 1 (next tap is Start); a multi-form This machine opens
 *  step 2. `resetSteps` (dialog open/close) and `back` (the footer) return to step 1. */
function usePickerSteps(runOn: RunTarget, setRunOn: (t: RunTarget) => void, localMultiForm: boolean) {
  const [step, setStep] = useState<"platform" | "form">("platform");
  const toPlatform = () => {
    setStep("platform");
  };
  const pickPlatform = (p: Platform) => {
    if (p.kind === "host") {
      setRunOn({ kind: "host", id: p.id });
      setStep("platform");
      return;
    }
    if (!isLocalTarget(runOn)) setRunOn({ kind: "local" });
    setStep(localMultiForm ? "form" : "platform");
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
