"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { attackVmStart, attackVmStatus, attackVmStop, exegolStatus, exegolStop, type ExegolStatus } from "@/lib/tauri";
import { startContainerAttackBox } from "@/features/labs/attack-box-auto";
import { attackImageLabel, getAttackBox, getAttackImage, getAutoAttackBox } from "@/lib/settings";
import { translate } from "@/lib/i18n";

/** What the attack box is: a container on a container lab's networks, or the learner's own VM
 *  (a Vagrant box) beside a VM lab, on the same hypervisor. */
type AttackBoxKind = "container" | "vm";

/** The attack box's last status: read now and polled every 5 s while the lab is up here
 *  (`active` and `local`), dropped when the lab starts or ends an operation. */
function useAttackBoxStatus(labId: string, kind: AttackBoxKind, active: boolean, local: boolean) {
  const [status, setStatus] = useState<ExegolStatus | null>(null);
  // A status read before an operation says nothing about after it (a shut down stops the box),
  // so drop it: the auto-start then waits for a fresh read instead of acting on it.
  const [wasActive, setWasActive] = useState(active);
  if (wasActive !== active) {
    setWasActive(active);
    setStatus(null);
  }
  // Only the newest status read wins: a slow poll that was already in flight when the box was
  // stopped must not resolve afterwards with a stale "running" and flip the button back (which
  // made Stop seem to need two or three clicks).
  const seq = useRef(0);
  // Reads in flight: the poll skips a tick while one is (a `vagrant status` can take longer
  // than the interval, and stacking them only slowed every one down).
  const reading = useRef(0);
  const refresh = useCallback(() => {
    const mine = ++seq.current;
    reading.current++;
    (kind === "vm" ? attackVmStatus(labId, getAttackBox()) : exegolStatus(labId, getAttackImage()))
      .then((s) => {
        if (mine === seq.current) setStatus(s);
      })
      // A failed read (Vagrant busy or slow, an emulated lab) says nothing about the box: keep
      // what was last seen rather than flip it to "not started".
      .catch(() => {})
      .finally(() => {
        reading.current--;
      });
  }, [labId, kind]);
  useEffect(() => {
    if (!active || !local) {
      // A read still in flight belongs to the run that just ended: ignore it.
      seq.current++;
      return;
    }
    refresh();
    const t = setInterval(() => {
      if (reading.current === 0) refresh();
    }, 5000);
    return () => clearInterval(t);
  }, [active, local, refresh]);
  return { status, setStatus, refresh };
}

/** A start or stop of the box, with its streamed log; the status is read again after it. */
function useAttackBoxRun(refresh: () => void) {
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<string[]>([]);
  const run = useCallback(
    async (fn: (onLog: (l: string) => void) => Promise<void>, first: string) => {
      setBusy(true);
      setLog([first]);
      try {
        await fn((line) => setLog((l) => [...l, line]));
      } catch (e) {
        setLog((l) => [...l, `✗ ${String(e)}`]);
      } finally {
        setBusy(false);
        refresh();
      }
    },
    [refresh],
  );
  return { busy, log, run };
}

/** Starts the box with the lab when Settings say so, once per run of the lab. Only a lab that is
 *  really down starts a new run: an operation in flight doesn't, nor does the stale "running"
 *  the lab status still shows just after one ends (that window re-created the box right after a
 *  shut down, wiping it). A box seen running counts as started, so stopping it by hand doesn't
 *  bring it back. Never beside a lab whose last deploy failed (its VMs may be up, the lab isn't
 *  ready). */
function useAutoStart(
  { running, holding, active, local, failed }: { running: boolean; holding: boolean; active: boolean; local: boolean; failed: boolean },
  status: ExegolStatus | null,
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
 * current), started and stopped with a streamed log, and started with the lab when the
 * Settings preference is on (once per run). `kind` picks the container (image from Settings)
 * or the VM (box from Settings); both report the same status shape. `holding` = a lab
 * operation (start, shut down, stop…) is in flight: nothing is read or started meanwhile.
 */
export function useAttackBox(
  labId: string,
  {
    running,
    holding = false,
    local,
    kind = "container",
    failed = false,
  }: { running: boolean; holding?: boolean; local: boolean; kind?: AttackBoxKind; failed?: boolean },
) {
  const active = running && !holding;
  const { status, setStatus, refresh } = useAttackBoxStatus(labId, kind, active, local);
  const { busy, log, run } = useAttackBoxRun(refresh);
  // The image or box in use, for the panel's label.
  const name = kind === "vm" ? getAttackBox() : attackImageLabel(getAttackImage());

  const start = useCallback(
    () =>
      run(
        (l) => (kind === "vm" ? attackVmStart(labId, getAttackBox(), l) : startContainerAttackBox(labId, l)),
        kind === "vm" ? translate("labs.attackBox.startingVm") : translate("labs.attackBox.startingBox"),
      ),
    [run, labId, kind],
  );
  const stop = useCallback(
    () =>
      run(async (l) => {
        await (kind === "vm" ? attackVmStop(labId, l) : exegolStop(labId, l));
        // It's gone now: show it immediately (a later poll confirms), so the button flips on
        // the first click instead of waiting on the next status read.
        setStatus((s) => (s ? { ...s, running: false, ip: "" } : s));
      }, translate("labs.attackBox.removing")),
    [run, labId, kind, setStatus],
  );
  useAutoStart({ running, holding, active, local, failed }, status, busy, start);

  return { status, busy, log, start, stop, kind, name };
}

export type AttackBox = ReturnType<typeof useAttackBox>;
