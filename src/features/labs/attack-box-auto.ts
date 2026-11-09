"use client";

import { useEffect, useRef } from "react";
import { attackBoxStatus, labStatus, type ActiveOperation, type Runtime } from "@/lib/tauri";
import { getAutoAttackBox } from "@/lib/settings";
import { tell } from "@/lib/failure";
import { translate } from "@/lib/i18n";
import { getLabState } from "@/features/labs/lab-store";
import { attachAttackBox } from "@/features/labs/attack-box-attach";

/** The runtime lab `id` declares, from the catalogue last loaded: its status and its box are read
 *  in it. A lab that catalogue doesn't list (not loaded yet) is taken for a container lab. */
function runtimeOf(id: string): Runtime {
  return getLabState().catalogue?.labs.find((l) => l.id === id)?.runtime?.runtime ?? "DOCKER";
}

/**
 * "Start it with each lab", wherever the lab was started: when a launch or resume ends with the
 * lab running, its attack box is added too, wherever the lab runs (here, on a server or in a cloud
 * account; a container among a container lab's, a Kali VM among a VM lab's). The lab page alone
 * did this before, so a lab started from Overview, the Labs list or the website ran without one
 * until its page was opened. Only where the box's status says the launcher reaches a run to put
 * it in (`reachable`: not on an ESXi host, nor beside a cloud-services lab). A failed attach is
 * told, as the lab page's own Start attack box tells it in its log.
 */
export function useAttackBoxAutoStart(ops: Map<string, ActiveOperation>, enabled: boolean) {
  const previous = useRef<Map<string, ActiveOperation["op"]>>(new Map());
  useEffect(() => {
    const ended = [...previous.current].filter(([id, op]) => (op === "launch" || op === "resume") && !ops.has(id)).map(([id]) => id);
    previous.current = new Map([...ops].map(([id, o]) => [id, o.op]));
    if (!enabled || ended.length === 0 || !getAutoAttackBox()) return;
    for (const id of ended) {
      const runtime = runtimeOf(id);
      labStatus(id, runtime)
        .then(async (s) => {
          // A failed start leaves nothing running.
          if (!s.running) return;
          const box = await attackBoxStatus(id, runtime);
          if (box.reachable && !box.running) await attachAttackBox(id, runtime, null);
        })
        .catch(tell(translate("labs.attackBox.autoAttachFailed")));
    }
  }, [ops, enabled]);
}
