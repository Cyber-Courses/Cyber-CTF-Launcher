"use client";

import { lazy, Suspense } from "react";
import { WDIO_HOOKS } from "@/lib/wdio";

const LaunchHook = WDIO_HOOKS ? lazy(() => import("@/components/wdio-launch").then((m) => ({ default: m.WdioLaunch }))) : null;
const AttackHook = WDIO_HOOKS ? lazy(() => import("@/components/wdio-attack").then((m) => ({ default: m.WdioAttack }))) : null;

/** The GUI e2e's hooks (`WdioLaunch`, `WdioAttack`), in a build that carries them; nothing
 *  otherwise. */
export function WdioHooks() {
  if (!LaunchHook || !AttackHook) return null;
  return (
    <Suspense fallback={null}>
      <LaunchHook />
      <AttackHook />
    </Suspense>
  );
}
