"use client";

import { useSyncExternalStore } from "react";
import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { useAttackBox } from "@/features/labs/use-attack-box";
import { useLabShell } from "@/features/labs/lab-detail/use-lab-shell";

/**
 * What a spec dispatches on `window` once it has set `__wdio_attack__`: the panel mounts then,
 * with no page reload (a reload right after launch ends the Windows app with a fast-fail).
 */
export const WDIO_ATTACK_EVENT = "cyberctf:wdio-attack";

/** Calls `changed` whenever a spec says the lab to mount the panel for changed. */
function subscribe(changed: () => void): () => void {
  window.addEventListener(WDIO_ATTACK_EVENT, changed);
  return () => {
    window.removeEventListener(WDIO_ATTACK_EVENT, changed);
  };
}

/** The lab id a test asked the attack-box panel to mount for. */
function wdioAttackLab(): string | null {
  try {
    return localStorage.getItem("__wdio_attack__");
  } catch {
    return null;
  }
}

/**
 * GUI e2e only: when localStorage `__wdio_attack__` holds a running local lab's id, mount the real
 * attack-box panel the lab page uses (the same `useAttackBox`, `AttackBoxPanel` and `useLabShell`),
 * so an e2e can attach the attack box, see it running, open the attacker shell and stop it — the
 * real panel, the real commands. A locally imported lab never reaches the catalogue detail page
 * where that panel otherwise lives, so this is the only way to drive it against a local lab.
 * Mounted only in a build that carries the e2e hooks (`WdioHooks`), never a release build.
 */
export function WdioAttack() {
  // Read at mount, and again whenever a spec dispatches WDIO_ATTACK_EVENT.
  const labId = useSyncExternalStore(subscribe, wdioAttackLab, () => null);
  if (!labId) return null;
  return <WdioAttackPanel labId={labId} />;
}

/** The real side-rail attack-box panel, wired for a running local Docker lab. */
function WdioAttackPanel({ labId }: { labId: string }) {
  const box = useAttackBox(labId, { running: true, runtime: "DOCKER" });
  const shell = useLabShell(labId, "attack box", "DOCKER");
  return (
    <div data-wdio-attack className="max-w-sm p-4">
      <AttackBoxPanel box={box} running host={null} onShell={shell.open} shellReady={!!box.status?.running} />
    </div>
  );
}
