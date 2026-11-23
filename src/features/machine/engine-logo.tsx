"use client";

import { LogoTile } from "@/components/ui/logo-tile";
import { ENGINES, type Engine } from "@/features/machine/setup-steps/engines";
import type { DockerEngine } from "@/lib/tauri";

// A container engine's brand mark, for every screen that names one: the setup's engine cards,
// the Machine page's rows, the lab page's host card.

/** Brand mark in a fixed square: an app icon (`tile`) fills it; a bare mark sits on a white logo
 *  tile (white in every mode) so dark artwork stays legible. */
export function EngineLogo({ engine, size = "md" }: { engine: Engine; size?: "sm" | "md" }) {
  return <LogoTile src={engine.logo} fill={engine.tile} size={size} />;
}

/** The logo of a Docker-compatible engine by id; none for one without a mark (Podman). */
export function EngineMark({ id, size = "md" }: { id: DockerEngine; size?: "sm" | "md" }) {
  const e = ENGINES.find((x) => x.id === id);
  return e ? <EngineLogo engine={e} size={size} /> : null;
}
