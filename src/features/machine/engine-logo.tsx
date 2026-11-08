"use client";

import { ENGINES, type Engine } from "@/features/machine/setup-steps/engines";
import type { DockerEngine } from "@/lib/tauri";

// A container engine's brand mark, for every screen that names one: the setup's engine cards,
// the Machine page's rows, the lab page's host card.

const BOX = { sm: "size-6 rounded-sm", md: "size-8 rounded-control" } as const;
const PAD = { sm: "p-1", md: "p-1.5" } as const;

/** Brand mark in a fixed square. App icons (`tile`) fill it; bare marks sit on a white logo
 *  tile (white in every mode) so dark artwork stays legible. */
export function EngineLogo({ engine, size = "md" }: { engine: Engine; size?: "sm" | "md" }) {
  return engine.tile ? (
    // eslint-disable-next-line @next/next/no-img-element -- static export, plain asset
    <img src={engine.logo} alt="" className={`${BOX[size]} shrink-0 object-contain`} />
  ) : (
    <span className={`logo-tile flex ${BOX[size]} ${PAD[size]} shrink-0 items-center justify-center`}>
      {/* eslint-disable-next-line @next/next/no-img-element -- static export, plain asset */}
      <img src={engine.logo} alt="" className="size-full object-contain" />
    </span>
  );
}

/** The logo of a Docker-compatible engine by id; none for one without a mark (Podman). */
export function EngineMark({ id, size = "md" }: { id: DockerEngine; size?: "sm" | "md" }) {
  const e = ENGINES.find((x) => x.id === id);
  return e ? <EngineLogo engine={e} size={size} /> : null;
}
