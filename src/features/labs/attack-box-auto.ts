"use client";

import { useEffect, useRef } from "react";
import { attackBoxAttach, attackBoxStatus, labStatus, type ActiveOperation } from "@/lib/tauri";
import { getAttackImage, getAutoAttackBox } from "@/lib/settings";
import { startOnce } from "@/lib/start-once";
import { warn } from "@/lib/failure";

/** Hot-adds the attack box into a container lab here, or joins the attach already under way (a
 *  second attach would race it on the same run). The runner picks the only lab network. */
export const attachContainerBox = (labId: string, onLog: (line: string) => void = () => {}) =>
  startOnce(`attach:${labId}`, (log) => attackBoxAttach(labId, "DOCKER", null, getAttackImage(), log), onLog);

/**
 * "Start it with each lab", wherever the lab was started: when a launch or resume ends with
 * the lab's containers running here, its attack box is attached too. The lab page alone did this
 * before, so a lab started from Overview, the Labs list or the website ran without one until
 * its page was opened.
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
          // A failed start leaves nothing running; a lab on a server brought its box with it.
          if (!s.running || s.place !== "container") return;
          const box = await attackBoxStatus(id, "DOCKER", null);
          if (!box.running) await attachContainerBox(id);
        })
        .catch(warn("attaching the attack box with the lab"));
    }
  }, [ops, enabled]);
}
