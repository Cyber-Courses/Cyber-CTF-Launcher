"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { attackBoxDetach, attackBoxStatus, type AttackStatus, type Runtime } from "@/lib/tauri";
import { attackImageLabel, getAttackImage, getAutoAttackBox } from "@/lib/settings";
import { attachAttackBox } from "@/features/labs/attack-box-attach";
import { translate } from "@/lib/i18n";
import { ignore } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";
import { structurallyEqual } from "@/lib/equal";
import { useLatest } from "@/lib/use-latest";
import { useRunLog } from "@/lib/use-run-log";

/** The attack box's last status: read now and polled every 5 s while the lab is up (`active`)
 *  and the backend reaches a run of it to put a box in (its status's `reachable`: polling stops at
 *  the first read that says no), dropped when the lab starts or ends an operation. `network` is
 *  the one the box should report its address on (null: its first). */
function useAttackBoxStatus(labId: string, runtime: Runtime, active: boolean, network: string | null) {
  const [status, setStatus] = useState<AttackStatus | null>(null);
  // A status read before an operation says nothing about after it (a detach takes the box out),
  // so drop it: the auto-attach then waits for a fresh read instead of acting on it.
  const [wasActive, setWasActive] = useState(active);
  if (wasActive !== active) {
    setWasActive(active);
    setStatus(null);
  }
  // Only the newest status read wins: a slow poll that was already in flight when the box was
  // taken out must not resolve afterwards with a stale "running" and flip the button back (which
  // made Stop seem to need two or three clicks).
  const begin = useLatest();
  const read = useCallback(() => {
    const isLatest = begin();
    return attackBoxStatus(labId, runtime, network).then((s) => {
      // The same answer again keeps the current object: the page doesn't re-render.
      if (isLatest()) setStatus((prev) => (structurallyEqual(prev, s) ? prev : s));
    });
  }, [labId, runtime, network, begin]);
  // A failed read (the runner busy or slow, an emulated lab) says nothing about the box: keep what
  // was last seen rather than flip it to "not started".
  const refresh = useCallback(() => void read().catch(ignore("the box's status is read again in a moment")), [read]);
  const reachable = status?.reachable !== false;
  // Polled while the lab is up, one read at a time, paused while the window is hidden.
  usePoll(read, 5000, { enabled: active && reachable, restartKey: read, onValue: () => {}, onError: ignore("the box's status is read again in a moment") });
  useEffect(() => {
    // A read still in flight belongs to the run that just ended: ignore it.
    if (!active || !reachable) begin();
  }, [active, reachable, begin]);
  return { status, setStatus, refresh };
}

/** An attach or detach of the box, with its streamed log; the status is read again after it. */
function useAttackBoxRun(refresh: () => void) {
  const [busy, setBusy] = useState(false);
  const { lines, run: runLog } = useRunLog();
  const run = useCallback(
    async (fn: (onLog: (l: string) => void) => Promise<void>, first: string) => {
      setBusy(true);
      await runLog(first, fn);
      setBusy(false);
      refresh();
    },
    [runLog, refresh],
  );
  return { busy, log: lines ?? [], run };
}

/** Attaches the box with the lab when Settings say so, once per run of the lab. Only a lab that is
 *  really down starts a new run: an operation in flight doesn't, nor does the stale "running"
 *  the lab status still shows just after one ends. A box seen running counts as attached, so
 *  taking it out by hand doesn't bring it back. Never beside a lab whose last deploy failed. */
function useAutoStart(
  { running, holding, active, failed }: { running: boolean; holding: boolean; active: boolean; failed: boolean },
  status: AttackStatus | null,
  busy: boolean,
  start: () => Promise<void>,
) {
  // Set in Settings; read once per visit.
  const [autoStart] = useState(() => getAutoAttackBox());
  const autoStarted = useRef(false);
  useEffect(() => {
    if (!running && !holding) {
      autoStarted.current = false;
      return;
    }
    if (!active || !status) return;
    if (status.running) {
      autoStarted.current = true;
      return;
    }
    if (!autoStart || !status.reachable || busy || failed || autoStarted.current) return;
    autoStarted.current = true;
    void start();
  }, [running, holding, active, autoStart, status, busy, failed, start]);
}

/**
 * The lab's attack box, a machine the launcher adds to the lab (at the seat the lab declares for
 * the player, else on the network picked): polled while the lab is up where the launcher reaches
 * its run (the status says: wherever the lab runs but an ESXi host, and never beside a
 * cloud-services lab), so running / IP stay current,
 * added and taken out with a streamed log, and added with the lab when the Settings preference is
 * on (once per run). `runtime` is the lab's declared one; what the box is comes from its status
 * (`vm`: a Kali VM among a lab running as VMs, a container lab run as one VM per machine included;
 * otherwise a container from the attack box image in Settings). The learner picks which lab
 * network to join when the backend offers more than one (`networks`: none when the lab declares
 * where the player sits), `defaultNetwork` until they do. `holding` = a lab operation (start,
 * shut down, stop…) is in flight: nothing is read or attached meanwhile.
 */
export function useAttackBox(
  labId: string,
  { running, holding = false, runtime = "DOCKER", failed = false }: { running: boolean; holding?: boolean; runtime?: Runtime; failed?: boolean },
) {
  const active = running && !holding;
  // The lab network the box joins: the learner's pick (null: the seat the lab declares, else its
  // first network).
  const [network, setNetwork] = useState<string | null>(null);
  const { status, setStatus, refresh } = useAttackBoxStatus(labId, runtime, active, network);
  const { busy, log, run } = useAttackBoxRun(refresh);
  // Until the status says, the lab's declared runtime stands in: a VM lab's box is a VM.
  const vm = status?.vm ?? runtime === "VM";
  // What the box runs, for the panel's label: Kali among VMs, else the attack image.
  const name = vm ? "Kali Linux" : attackImageLabel(getAttackImage());

  const start = useCallback(
    () => run((l) => attachAttackBox(labId, runtime, network, l), translate("labs.attackBox.attaching")),
    [run, labId, runtime, network],
  );
  const stop = useCallback(
    () =>
      run(async (l) => {
        await attackBoxDetach(labId, runtime, l);
        // It's gone now: show it immediately (a later poll confirms), so the button flips on
        // the first click instead of waiting on the next status read.
        setStatus((s) => (s ? { ...s, running: false, ip: "" } : s));
      }, translate("labs.attackBox.removing")),
    [run, labId, runtime, setStatus],
  );
  useAutoStart({ running, holding, active, failed }, status, busy, start);

  const defaultNetwork = status?.defaultNetwork ?? "";
  return { status, busy, log, start, stop, vm, name, networks: status?.networks ?? [], network, defaultNetwork, setNetwork };
}

export type AttackBox = ReturnType<typeof useAttackBox>;
