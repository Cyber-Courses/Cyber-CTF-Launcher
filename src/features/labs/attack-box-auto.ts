"use client";

import { useEffect, useRef } from "react";
import { attackBoxStatus, labStatus, type ActiveOperation, type Runtime } from "@/lib/tauri";
import { getAutoAttackBox } from "@/lib/settings";
import { tell } from "@/lib/failure";
import { translate } from "@/lib/i18n";
import { getLabState } from "@/features/labs/lab-store";
import { attachAttackBox } from "@/features/labs/attack-box-attach";

/** The runtime lab `id` runs in: the one its worker's operation was run in (the backend's), else
 *  the one the catalogue last loaded declares for it; null when neither says (an operation in this
 *  process on a lab the catalogue doesn't list yet): nothing is guessed. */
function runtimeOf(id: string, op: ActiveOperation): Runtime | null {
  return op.runtime ?? getLabState().catalogue?.labs.find((l) => l.id === id)?.runtime?.runtime ?? null;
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
  const previous = useRef<Map<string, ActiveOperation>>(new Map());
  useEffect(() => {
    const ended = [...previous.current].filter(([id, o]) => (o.op === "launch" || o.op === "resume") && !ops.has(id));
    previous.current = new Map(ops);
    if (!enabled || ended.length === 0 || !getAutoAttackBox()) return;
    for (const [id, op] of ended) {
      const runtime = runtimeOf(id, op);
      if (!runtime) continue;
      labStatus(id, runtime)
        .then(async (s) => {
          // A failed launch or resume reads as not running (the backend's status says so).
          if (!s.running) return;
          const box = await attackBoxStatus(id, runtime);
          if (box.reachable && !box.running) await attachAttackBox(id, runtime, null);
        })
        .catch(tell(translate("labs.attackBox.autoAttachFailed")));
    }
  }, [ops, enabled]);
}
