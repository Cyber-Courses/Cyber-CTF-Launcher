"use client";

import { useEffect, useState } from "react";
import { ignore } from "@/lib/failure";
import { type Badge, type Provider, type RunTier, runTiers } from "@/lib/tauri";
import { CLOUDS } from "@/features/labs/lab-runtime";
import type { RunTarget } from "@/features/labs/run-on";

// The maturity badge (Beta / Experimental) on each place of "Where should it run?", from
// Isoloom's tier evidence for the launch paths a place could take.

/** A maturity badge on a launch path (Beta / Experimental), with the evidence as its tooltip. */
export type RunBadge = { kind: Exclude<Badge, "none">; title: string };

/** Every launch path's maturity, fetched once when the picker opens (empty while it loads, or
 *  when the evidence can't be read — then no badges show). */
export function useRunTiers(): RunTier[] {
  const [tiers, setTiers] = useState<RunTier[]>([]);
  useEffect(() => {
    let live = true;
    runTiers()
      .then((t) => {
        if (live) setTiers(t);
      })
      .catch(ignore("without the evidence, no launch path shows a badge"));
    return () => {
      live = false;
    };
  }, []);
  return tiers;
}

const BADGE_RANK: Record<Badge, number> = { none: 0, beta: 1, experimental: 2 };

/** The badge a launch path carries: the strongest (experimental > beta) of its candidate
 *  `target/variant` entries that has one. `None` → no badge. Purely from Isoloom's evidence. */
export function badgeFor(tiers: RunTier[], candidates: string[]): RunBadge | undefined {
  let worst: RunTier | undefined;
  for (const key of candidates) {
    const slash = key.indexOf("/");
    const [target, variant] = slash === -1 ? [key, null] : [key.slice(0, slash), key.slice(slash + 1)];
    const entry = tiers.find((e) => e.target === target && (e.variant ?? null) === variant);
    if (entry && entry.badge !== "none" && (!worst || BADGE_RANK[entry.badge] > BADGE_RANK[worst.badge])) worst = entry;
  }
  return worst && worst.badge !== "none" ? { kind: worst.badge, title: worst.evidence } : undefined;
}

/** The Isoloom `target/variant` paths a run target could take (so the worst of them badges it). */
export function candidatesFor(target: RunTarget, provider?: Provider): string[] {
  switch (target.kind) {
    case "local":
      return ["docker"];
    case "local-vm":
      return [`docker-vm/${target.provider}`, `vagrant/${target.provider}`];
    case "local-vms":
      return [`vagrant/${target.provider}`];
    case "host":
      if (!provider) return [];
      if (CLOUDS.has(provider)) return [`cloud-vm/${provider}`, `cloud-docker/${provider}`];
      if (provider === "vmware_esxi") return ["vagrant/vmware_esxi"];
      return [`cloud-vm/${provider}`, `vagrant/${provider}`];
  }
}
