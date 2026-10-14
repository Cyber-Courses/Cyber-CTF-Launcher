"use client";

import { useEffect, useRef } from "react";
import { exegolStart, exegolStatus, labStatus, type ActiveOperation } from "@/lib/tauri";
import { getAttackImage, getAutoAttackBox } from "@/lib/settings";
import { startOnce } from "@/lib/start-once";
import { warn } from "@/lib/failure";

/** Starts the container attack box beside a lab here, or joins the start already under way (a
 *  second `docker run` of the same name would fail). */
export const startContainerAttackBox = (labId: string, onLog: (line: string) => void = () => {}) =>
  startOnce(`container:${labId}`, () => exegolStart(labId, getAttackImage(), onLog));

/**
 * "Start it with each lab", wherever the lab was started: when a launch or resume ends with
 * the lab's containers running here, its attack box starts too. The lab page alone did this
 * before, so a lab started from Overview, the Labs list or the website ran without one until
 * its page was opened. VM labs bring theirs with the deploy itself.
 */
export function useAttackBoxAutoStart(ops: Map<string, ActiveOperation>, enabled: boolean) {
  const previous = useRef<Map<string, ActiveOperation["op"]>>(new Map());
  useEffect(() => {
    const ended = [...previous.current].filter(([id, op]) => (op === "launch" || op === "resume") && !ops.has(id)).map(([id]) => id);
    previous.current = new Map([...ops].map(([id, o]) => [id, o.op]));
    if (!enabled || ended.length === 0 || !getAutoAttackBox()) return;
    for (const id of ended) {
      labStatus(id, "DOCKER")
        .then(async (s) => {
          // A failed start leaves nothing running; a lab inside a local VM or on a server brought its box with it.
          if (!s.running || s.place !== "container") return;
          const box = await exegolStatus(id, getAttackImage());
          if (!box.running) await startContainerAttackBox(id);
        })
        .catch(warn("starting the attack box with the lab"));
    }
  }, [ops, enabled]);
}
