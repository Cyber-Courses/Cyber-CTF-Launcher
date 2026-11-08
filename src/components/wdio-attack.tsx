"use client";

import { useState } from "react";
import { AttackBoxPanel } from "@/features/labs/attack-box-panel";
import { useAttackBox } from "@/features/labs/use-attack-box";
import { useLabShell } from "@/features/labs/lab-detail/use-lab-shell";

/** The lab id a test asked the attack-box panel to mount for, read once. */
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
 * where that panel otherwise lives, so this is the only way to drive it against a local lab. Inert
 * unless a test sets the flag, which the shipped app never does.
 */
export function WdioAttack() {
  // Read once at mount (like the Settings-driven flags), so there is no setState in an effect.
  const [labId] = useState<string | null>(wdioAttackLab);
  if (!labId) return null;
  return <WdioAttackPanel labId={labId} />;
}

/** The real side-rail attack-box panel, wired for a running local Docker lab. */
function WdioAttackPanel({ labId }: { labId: string }) {
  const box = useAttackBox(labId, { running: true, reachable: true, runtime: "DOCKER" });
  const shell = useLabShell(labId, "attack box", "DOCKER");
  return (
    <div data-wdio-attack className="max-w-sm p-4">
      <AttackBoxPanel box={box} running host={null} onShell={shell.open} shellReady={!!box.status?.running} />
    </div>
  );
}
