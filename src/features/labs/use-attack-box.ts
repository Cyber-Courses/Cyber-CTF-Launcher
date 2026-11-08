"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { attackBoxAttach, attackBoxDetach, attackBoxStatus, type AttackStatus, type Runtime } from "@/lib/tauri";
import { startOnce } from "@/lib/start-once";
import { attackImageLabel, getAttackBoxChoice, getAttackImage, getAutoAttackBox } from "@/lib/settings";
import { translate } from "@/lib/i18n";
import { ignore } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";
import { structurallyEqual } from "@/lib/equal";
import { useLatest } from "@/lib/use-latest";
import { useRunLog } from "@/features/servers/host-setup/use-run-log";

/** What the attach sends, from the box's status: the Vagrant box from Settings when the attacker is
 *  a VM lab's own attack VM (`pickedBox`; null until one is picked: the lab's default box for this
 *  machine's CPU runs), else the attack box image, which runs beside a container lab's containers
 *  or inside the attack VM the runner adds to a lab running as VMs. */
const boxImage = (pickedBox: boolean) => (pickedBox ? getAttackBoxChoice() : getAttackImage());

/** Attaches the attack box onto `network` (null: the only one), or joins the attach already under
 *  way: a second attach would race the first on the same run. */
const attach = (labId: string, runtime: Runtime, network: string | null, pickedBox: boolean, onLog: (line: string) => void) =>
  startOnce(`attach:${labId}`, (log) => attackBoxAttach(labId, runtime, network, boxImage(pickedBox), log), onLog);

/** The attack box's last status: read now and polled every 5 s while the lab is up where the
 *  launcher reaches its box (`active` and `reachable`), dropped when the lab starts or ends an
 *  operation. `network` is the one the box should report its address on (null: its first). */
function useAttackBoxStatus(labId: string, runtime: Runtime, active: boolean, reachable: boolean, network: string | null) {
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
  { running, holding, active, reachable, failed }: { running: boolean; holding: boolean; active: boolean; reachable: boolean; failed: boolean },
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
    if (!autoStart || !reachable || busy || failed || autoStarted.current) return;
    autoStarted.current = true;
    void start();
  }, [running, holding, active, autoStart, reachable, status, busy, failed, start]);
}

/**
 * The lab's attack box: polled while the lab is up where the launcher reaches its box
 * (`reachable`: on this machine, or a container lab on a server or in a cloud account, whose box
 * is a container in the lab's VM there), so running / IP stay current, attached and detached through the runner with a streamed log, and attached with the
 * lab when the Settings preference is on (once per run). `runtime` is the lab's declared one; what
 * the box is here comes from its status (`vm`: a VM beside a lab running as VMs on this machine,
 * a container lab run as one VM per machine included; `pickedBox`: the lab's own attack VM, booted
 * from the Vagrant box in Settings; otherwise the attack box image from Settings). The learner
 * picks which lab network to join when the lab has more than one (`networks`). `holding` = a lab
 * operation (start, shut down, stop…) is in flight: nothing is read or attached meanwhile.
 */
export function useAttackBox(
  labId: string,
  {
    running,
    holding = false,
    reachable,
    runtime = "DOCKER",
    failed = false,
  }: { running: boolean; holding?: boolean; reachable: boolean; runtime?: Runtime; failed?: boolean },
) {
  const active = running && !holding;
  // The lab network the box joins: the learner's pick, or the only one (null = the runner's choice).
  const [network, setNetwork] = useState<string | null>(null);
  const { status, setStatus, refresh } = useAttackBoxStatus(labId, runtime, active, reachable, network);
  const { busy, log, run } = useAttackBoxRun(refresh);
  // Until the status says, the lab's declared runtime stands in: a VM lab's box is a VM.
  const vm = status?.vm ?? runtime === "VM";
  const pickedBox = status?.pickedBox ?? false;
  // The image or box in use, for the panel's label: the lab's own default box until one is picked.
  const name = pickedBox ? (getAttackBoxChoice() ?? translate("labs.attackBox.defaultBox")) : attackImageLabel(getAttackImage());

  const start = useCallback(
    () => run((l) => attach(labId, runtime, network, pickedBox, l), translate("labs.attackBox.attaching")),
    [run, labId, runtime, network, pickedBox],
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
  useAutoStart({ running, holding, active, reachable, failed }, status, busy, start);

  return { status, busy, log, start, stop, vm, name, networks: status?.networks ?? [], network, setNetwork };
}

export type AttackBox = ReturnType<typeof useAttackBox>;
