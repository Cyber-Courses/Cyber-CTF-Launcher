"use client";

import { lazy, Suspense } from "react";

/** Whether this build carries the WebDriver e2e hooks: a development build (the desktop e2e's
 *  debug app loads `next dev`), or one built with `NEXT_PUBLIC_WDIO=1` (a release app under the
 *  e2e). Both are inlined at build time: a production build without the flag drops the hooks
 *  and their code. */
const WDIO_HOOKS = process.env.NODE_ENV !== "production" || process.env.NEXT_PUBLIC_WDIO === "1";

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
