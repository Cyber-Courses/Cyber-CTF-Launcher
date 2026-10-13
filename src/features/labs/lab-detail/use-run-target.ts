"use client";

import { useCallback, useEffect, useState } from "react";
import { cloudAccountsFor, hostSupports, isCloudLab, localProviders } from "@/features/labs/lab-runtime";
import type { RunTarget } from "@/features/labs/run-on";
import type { LabRuntimeInfo } from "@/features/labs/use-labs";
import { getPortMode, type PortMode } from "@/lib/settings";
import { SERVER_CHANGED, serverList, type Provider, type ServerHost } from "@/lib/tauri";
import { useTauriEvent } from "@/lib/use-tauri-event";

/**
 * Where a lab starts, picked in "Where should it run?": this machine or one of the player's
 * server hosts (Docker labs through their deploy/ layer), a VM here for a container lab, or
 * hosted by Cyber CTF. VM labs default to the default host; Docker labs to here; a cloud lab
 * to the first of the player's accounts on a cloud it runs on (never a default host: a server).
 */
export function useRunTarget(rt: LabRuntimeInfo | null, readyVms: Provider[], hostArch: string) {
  const isCloud = isCloudLab(rt);
  const isDocker = rt?.runtime !== "VM" && !isCloud;
  const [hosts, setHosts] = useState<ServerHost[]>([]);
  const [runOn, setRunOn] = useState<RunTarget>({ kind: "local" });
  // A container lab here: its host ports (Settings' choice first).
  const [ports, setPorts] = useState<PortMode>(() => getPortMode() ?? "random");
  // A container lab can also run in a VM here: on the hypervisor chosen in Settings (readyVms
  // lists it first), else the first ready one its deploy/ supports. One option, not a catalogue.
  const localVm = isDocker ? (readyVms.find((p) => rt?.providers.includes(p)) ?? null) : null;
  const hostOk = useCallback((h: ServerHost) => hostSupports(rt, h), [rt]);
  // Prefer this machine when a local hypervisor can run the lab; fall back to the default server
  // only when none can. A default server shouldn't silently capture every VM lab.
  const localReady = isDocker || (!!rt && readyVms.some((p) => localProviders(rt, hostArch).includes(p)));
  const [serversChanged, setServersChanged] = useState(0);
  useEffect(() => {
    serverList()
      .then((l) => {
        setHosts(l.hosts);
        setRunOn(initialTarget(rt, l.hosts, l.default, localReady));
      })
      .catch(() => setHosts([]));
  }, [rt, localReady, serversChanged]);
  // A server or cloud account added (or removed) in its setup window: the page follows at once,
  // instead of asking to "Connect an account" until it is left and opened again.
  useTauriEvent(SERVER_CHANGED, () => setServersChanged((n) => n + 1));
  const cloudAccounts = cloudAccountsFor(rt, hosts);

  return {
    hosts,
    hostOk,
    runOn,
    setRunOn,
    ports,
    setPorts,
    localVm,
    /** A cloud lab: the player's accounts it can run on, and the one picked. */
    cloudAccounts,
    pickedAccount: runOn.kind === "host" ? cloudAccounts.find((h) => h.id === runOn.id) : undefined,
    /** With servers saved (or a VM here, or hosted), Start asks where to run first; a cloud lab
     *  always asks (which account, and what it costs there). */
    hasChoice: isCloud || hosts.length > 0 || localVm !== null || !!rt?.hosted,
    /** The target picked, with the host ports for a container lab here. */
    chosen: (): RunTarget => (runOn.kind === "local" && isDocker ? { kind: "local", ports } : runOn),
  };
}

/** Where Start points at first: a cloud lab's first account; the default host when this machine
 *  can't run the lab (`localReady`) and the host can; else here. */
export function initialTarget(rt: LabRuntimeInfo | null, hosts: ServerHost[], defaultId: string | null, localReady: boolean): RunTarget {
  if (isCloudLab(rt)) {
    const account = cloudAccountsFor(rt, hosts)[0];
    return account ? { kind: "host", id: account.id } : { kind: "local" };
  }
  const def = hosts.find((h) => h.id === defaultId);
  return !localReady && def && hostSupports(rt, def) ? { kind: "host", id: def.id } : { kind: "local" };
}

export type RunTargetChoice = ReturnType<typeof useRunTarget>;
