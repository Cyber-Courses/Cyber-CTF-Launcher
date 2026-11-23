// How well each feature is checked: a mark per tier (covered, partial, none) and a level per
// feature (Strong, Partial, Unchecked), from what the registry says its tests are and leave out.
import { rigState, type RigSnapshot } from "./matrix.ts";
import { features, TIERS, type Feature, type Platform, type Registry, type Tier } from "./registry.ts";

export type Mark = "covered" | "partial" | "none";
export type Level = "Strong" | "Partial" | "Unchecked";

/** The tiers that run the real app or the real tools on a real machine. */
const REAL_HOST: readonly Tier[] = ["desktop", "integration", "setup", "package"];
/** The tiers that drive the app end to end, real or against the dev mock. */
const END_TO_END: readonly Tier[] = ["web", "desktop"];

/** A tier's mark: none without tests, partial when the registry says what they leave out. */
export function mark(f: Feature, tier: Tier): Mark {
  if (!f.tests[tier].length) return "none";
  return f.gaps[tier] ? "partial" : "covered";
}

/** Whether a rig cell the feature names has passed on one of its platforms (true when it names
 *  none: then nothing says where its real-host tests ran). */
function provenOnRig(f: Feature, rig: RigSnapshot): boolean {
  return !f.rig.length || rigPassed(f, rig).length > 0;
}

/**
 * A feature's level. Strong: unit tests, and a tier above them that covers it where it runs: a
 * real-host one for a feature that depends on the machine (the dev mock proves nothing about
 * Docker or a hypervisor), any end-to-end one for the app's own behaviour; and, when the rig runs
 * the feature, a cell of it passed there on one of its platforms (a real-host test never run is
 * coverage on paper). Unchecked: no test in any tier. Partial: everything between.
 */
export function level(f: Feature, rig: RigSnapshot): Level {
  const marks = Object.fromEntries(TIERS.map((t) => [t, mark(f, t)])) as Record<Tier, Mark>;
  if (TIERS.every((t) => marks[t] === "none")) return "Unchecked";
  if (marks.unit === "none") return "Partial";
  if (!f.host) return [...END_TO_END, ...REAL_HOST].some((t) => marks[t] === "covered") ? "Strong" : "Partial";
  return REAL_HOST.some((t) => marks[t] === "covered") && provenOnRig(f, rig) ? "Strong" : "Partial";
}

/** Each level's count among `list`. */
export function levelCounts(list: Feature[], rig: RigSnapshot): Record<Level, number> {
  const out: Record<Level, number> = { Strong: 0, Partial: 0, Unchecked: 0 };
  for (const f of list) out[level(f, rig)] += 1;
  return out;
}

/** Where a feature's rig cells last passed, of the platforms it runs on. */
export function rigPassed(f: Feature, rig: RigSnapshot): Platform[] {
  return f.platforms.filter((p) => rigState(rig, f.rig, p) === "pass");
}

export type Gap = { title: string; detail: string; features: string[] };

/** Why a feature lacks a real-host check: the tiers' own notes, or that none exists. */
function realHostWhy(f: Feature): string {
  const notes = REAL_HOST.map((t) => f.gaps[t]).filter(Boolean);
  return notes.length ? notes.join("; ") : "no desktop, integration, setup or package test";
}

/** A feature's gap: its id and what it lacks, then why (its description is in its area's table). */
const lacking = (f: Feature, title: string, detail: string): Gap => ({ title: `${f.id}: ${title}`, detail, features: [] });

/** The gaps worth a line at the top, worst first: the owner's known ones; the features no test
 *  checks (those that need the machine first); the machine's features with no real-host test at
 *  all; those whose real-host tests never passed on the rig; then how far the rig reaches. At most
 *  `limit`. */
export function biggestGaps(registry: Registry, rig: RigSnapshot, limit: number): Gap[] {
  const list = features(registry);
  const known: Gap[] = registry.knownGaps.map((g) => ({ title: g.what, detail: g.status, features: g.features }));
  const unchecked = list
    .filter((f) => level(f, rig) === "Unchecked")
    .sort((a, b) => Number(b.host) - Number(a.host))
    .map((f) => lacking(f, "no test", f.note));
  const host = list.filter((f) => f.host && level(f, rig) === "Partial");
  const noRealHost = host.filter((f) => REAL_HOST.every((t) => mark(f, t) === "none")).map((f) => lacking(f, "no real-host test", realHostWhy(f)));
  const neverRan = host
    .filter((f) => REAL_HOST.some((t) => mark(f, t) === "covered") && !provenOnRig(f, rig))
    .map((f) => lacking(f, "its real-host tests never passed on the rig", `Rig cells: ${f.rig.join(", ")}.`));
  const unproven = list.filter((f) => f.host && f.rig.length && rigPassed(f, rig).length < f.platforms.length);
  const reach: Gap[] = unproven.length
    ? [
        {
          title: `${String(unproven.length)} features that depend on the machine have never passed on the rig on one of their platforms`,
          detail: `the rig's matrix (generated ${rig.generated || "never"}) has passing cells on ${platformsWithPasses(rig).join(", ") || "no platform"} only`,
          features: unproven.map((f) => f.id),
        },
      ]
    : [];
  return [...known, ...unchecked, ...reach, ...noRealHost, ...neverRan].slice(0, limit);
}

/** The platforms on which some rig cell passed. */
export function platformsWithPasses(rig: RigSnapshot): Platform[] {
  return (["linux", "macos", "windows"] as const).filter((p) => rigState(rig, rig.columns, p) === "pass");
}
