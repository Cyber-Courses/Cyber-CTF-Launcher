"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { attackBoxAttach, attackBoxDetach, attackBoxStatus, type AttackStatus, type Runtime } from "@/lib/tauri";
import { startOnce } from "@/lib/start-once";
import { attackImageLabel, getAttackBox, getAttackImage, getAutoAttackBox } from "@/lib/settings";
import { translate } from "@/lib/i18n";
import { ignore } from "@/lib/failure";
import { usePoll } from "@/lib/use-poll";
import { structurallyEqual } from "@/lib/equal";
import { useLatest } from "@/lib/use-latest";
import { describeError } from "@/lib/errors";

/** The box image the learner chose for this lab's runtime: a Vagrant box beside a VM lab, a
 *  container image beside a container lab. The runner takes either as its attach image. */
const boxImage = (runtime: Runtime) => (runtime === "VM" ? getAttackBox() : getAttackImage());

/** Attaches the attack box onto `network` (null: the only one), or joins the attach already under
 *  way: a second attach would race the first on the same run. */
const attach = (labId: string, runtime: Runtime, network: string | null, onLog: (line: string) => void) =>
  startOnce(`attach:${labId}`, (log) => attackBoxAttach(labId, runtime, network, boxImage(runtime), log), onLog);

/** The attack box's last status: read now and polled every 5 s while the lab is up here
 *  (`active` and `local`), dropped when the lab starts or ends an operation. `network` is the one
 *  the box should report its address on (null: its first). */
function useAttackBoxStatus(labId: string, runtime: Runtime, active: boolean, local: boolean, network: string | null) {
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
  // Polled while the lab is up here, one read at a time, paused while the window is hidden.
  usePoll(read, 5000, { enabled: active && local, restartKey: read, onValue: () => {}, onError: ignore("the box's status is read again in a moment") });
  useEffect(() => {
    // A read still in flight belongs to the run that just ended: ignore it.
    if (!active || !local) begin();
  }, [active, local, begin]);
  return { status, setStatus, refresh };
}

/** An attach or detach of the box, with its streamed log; the status is read again after it. */
function useAttackBoxRun(refresh: () => void) {
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<string[]>([]);
  const run = useCallback(
    async (fn: (onLog: (l: string) => void) => Promise<void>, first: string) => {
      setBusy(true);
      setLog([first]);
      try {
        await fn((line) => {
          setLog((l) => [...l, line]);
        });
      } catch (e) {
        setLog((l) => [...l, `✗ ${describeError(e)}`]);
      } finally {
        setBusy(false);
        refresh();
      }
    },
    [refresh],
  );
  return { busy, log, run };
}

/** Attaches the box with the lab when Settings say so, once per run of the lab. Only a lab that is
 *  really down starts a new run: an operation in flight doesn't, nor does the stale "running"
 *  the lab status still shows just after one ends. A box seen running counts as attached, so
 *  taking it out by hand doesn't bring it back. Never beside a lab whose last deploy failed. */
function useAutoStart(
  { running, holding, active, local, failed }: { running: boolean; holding: boolean; active: boolean; local: boolean; failed: boolean },
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
    if (!autoStart || !local || busy || failed || autoStarted.current) return;
    autoStarted.current = true;
    void start();
  }, [running, holding, active, autoStart, local, status, busy, failed, start]);
}

/**
 * The lab's attack box: polled while a lab is up on this machine (so running / IP stay
 * current), attached and detached through the runner with a streamed log, and attached with the
 * lab when the Settings preference is on (once per run). `runtime` is the lab's (a container
 * image beside a Docker lab, a Vagrant box beside a VM lab, both from Settings). The learner
 * picks which lab network to join when the lab has more than one (`networks`). `holding` = a lab
 * operation (start, shut down, stop…) is in flight: nothing is read or attached meanwhile.
 */
export function useAttackBox(
  labId: string,
  {
    running,
    holding = false,
    local,
    runtime = "DOCKER",
    failed = false,
  }: { running: boolean; holding?: boolean; local: boolean; runtime?: Runtime; failed?: boolean },
) {
  const active = running && !holding;
  // The lab network the box joins: the learner's pick, or the only one (null = the runner's choice).
  const [network, setNetwork] = useState<string | null>(null);
  const { status, setStatus, refresh } = useAttackBoxStatus(labId, runtime, active, local, network);
  const { busy, log, run } = useAttackBoxRun(refresh);
  const vm = runtime === "VM";
  // The image or box in use, for the panel's label.
  const name = vm ? getAttackBox() : attackImageLabel(getAttackImage());

  const start = useCallback(() => run((l) => attach(labId, runtime, network, l), translate("labs.attackBox.attaching")), [run, labId, runtime, network]);
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
  useAutoStart({ running, holding, active, local, failed }, status, busy, start);

  return { status, busy, log, start, stop, vm, name, networks: status?.networks ?? [], network, setNetwork };
}

export type AttackBox = ReturnType<typeof useAttackBox>;
