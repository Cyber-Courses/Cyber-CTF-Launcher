// The feature registry (`tests/features.yml`): what the launcher does for its user, area by area,
// and the tests that check each feature, tier by tier. This module reads it and holds it to its
// shape; whether what it names exists is `check.ts`'s, how well it is covered `coverage.ts`'s.
import { parse } from "yaml";

/** The test tiers, in the order the tables show them (tests/README.md, "The tiers"); the guided
 *  setup's headless run (`integration_tests::setup`) is one of its own, its items what it checks. */
export const TIERS = ["unit", "web", "desktop", "integration", "setup", "package"] as const;
export type Tier = (typeof TIERS)[number];

/** The platforms the launcher ships for. */
export const PLATFORMS = ["linux", "macos", "windows"] as const;
export type Platform = (typeof PLATFORMS)[number];

export type Feature = {
  id: string;
  area: string;
  what: string;
  platforms: Platform[];
  providers: string[];
  /** Whether what it does depends on the real machine (a tool, a hypervisor, a server, a cloud):
   *  then only a real-host tier (desktop, integration, setup) proves it, never the dev mock. */
  host: boolean;
  commands: string[];
  pages: string[];
  tests: Record<Tier, string[]>;
  rig: string[];
  /** Per tier, what its tests leave out: a tier with tests and a gap is partial. */
  gaps: Partial<Record<Tier, string>>;
  note: string;
};

export type Area = { id: string; title: string; features: Feature[] };

/** A gap the owner tracks, whatever the tables say: what is missing, the features it touches,
 *  and where it stands (a branch being built, planned, not planned). */
type KnownGap = { id: string; what: string; features: string[]; status: string };

export type Registry = { areas: Area[]; knownGaps: KnownGap[] };

type Obj = Record<string, unknown>;

const FEATURE_KEYS = ["id", "what", "platforms", "providers", "host", "commands", "pages", "tests", "rig", "gaps", "note"];
const ID = /^[a-z0-9]+(-[a-z0-9]+)*$/;

const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);

/** The keys of `o` not in `known`, each as a problem at `where`. */
function unknownKeys(o: Obj, known: readonly string[], where: string): string[] {
  return Object.keys(o)
    .filter((k) => !known.includes(k))
    .map((k) => `${where}: unknown key \`${k}\` (known: ${known.join(", ")})`);
}

/** A list of strings at `o[key]` (absent: empty); a problem when it is something else. */
function strings(o: Obj, key: string, where: string, problems: string[]): string[] {
  const v = o[key];
  if (v === undefined || v === null) return [];
  if (Array.isArray(v) && v.every((s) => typeof s === "string")) return v;
  problems.push(`${where}: \`${key}\` must be a list of strings`);
  return [];
}

/** A string at `o[key]` (absent: `fallback`, or a problem when there is none). */
function text(o: Obj, key: string, where: string, problems: string[], fallback?: string): string {
  const v = o[key];
  if (typeof v === "string" && v.trim()) return v.trim();
  if (v === undefined && fallback !== undefined) return fallback;
  problems.push(`${where}: \`${key}\` must be a non-empty string`);
  return fallback ?? "";
}

function readTests(f: Obj, where: string, problems: string[]): Record<Tier, string[]> {
  const raw = f.tests ?? {};
  const out = Object.fromEntries(TIERS.map((t) => [t, [] as string[]])) as Record<Tier, string[]>;
  if (!isObj(raw)) {
    problems.push(`${where}: \`tests\` must be a map of tier to tests`);
    return out;
  }
  problems.push(...unknownKeys(raw, TIERS, `${where} tests`));
  for (const tier of TIERS) out[tier] = strings(raw, tier, `${where} tests`, problems);
  return out;
}

function readGaps(f: Obj, where: string, problems: string[]): Partial<Record<Tier, string>> {
  const raw = f.gaps ?? {};
  if (!isObj(raw)) {
    problems.push(`${where}: \`gaps\` must be a map of tier to what is missing`);
    return {};
  }
  problems.push(...unknownKeys(raw, TIERS, `${where} gaps`));
  const out: Partial<Record<Tier, string>> = {};
  for (const tier of TIERS) if (raw[tier] !== undefined) out[tier] = text(raw, tier, `${where} gaps`, problems);
  return out;
}

function readPlatforms(f: Obj, where: string, problems: string[]): Platform[] {
  if (f.platforms === undefined) return [...PLATFORMS];
  const listed = strings(f, "platforms", where, problems);
  const bad = listed.filter((p) => !(PLATFORMS as readonly string[]).includes(p));
  if (bad.length) problems.push(`${where}: unknown platforms ${bad.join(", ")} (known: ${PLATFORMS.join(", ")})`);
  return PLATFORMS.filter((p) => listed.includes(p));
}

function readFeature(raw: unknown, area: string, index: number, problems: string[]): Feature | null {
  const at = `${area}[${String(index)}]`;
  if (!isObj(raw)) {
    problems.push(`${at}: a feature must be a map`);
    return null;
  }
  const id = text(raw, "id", at, problems);
  const where = id || at;
  problems.push(...unknownKeys(raw, FEATURE_KEYS, where));
  const dot = id.indexOf(".");
  if (id && (id.slice(0, dot) !== area || !ID.test(id.slice(dot + 1)))) problems.push(`${where}: an id is \`${area}.<kebab-case-name>\``);
  if (raw.host !== undefined && typeof raw.host !== "boolean") problems.push(`${where}: \`host\` must be true or false`);
  return {
    id,
    area,
    what: text(raw, "what", where, problems),
    platforms: readPlatforms(raw, where, problems),
    providers: strings(raw, "providers", where, problems),
    host: raw.host === true,
    commands: strings(raw, "commands", where, problems),
    pages: strings(raw, "pages", where, problems),
    tests: readTests(raw, where, problems),
    rig: strings(raw, "rig", where, problems),
    gaps: readGaps(raw, where, problems),
    note: text(raw, "note", where, problems, ""),
  };
}

function readArea(raw: unknown, index: number, problems: string[]): Area | null {
  const at = `areas[${String(index)}]`;
  if (!isObj(raw)) {
    problems.push(`${at}: an area must be a map`);
    return null;
  }
  problems.push(...unknownKeys(raw, ["id", "title", "features"], at));
  const id = text(raw, "id", at, problems);
  if (id && !ID.test(id)) problems.push(`${at}: an area's id is kebab-case`);
  const title = text(raw, "title", id || at, problems);
  const list = Array.isArray(raw.features) ? (raw.features as unknown[]) : [];
  if (!list.length) problems.push(`${id || at}: an area lists its features`);
  const features = list.map((f, i) => readFeature(f, id, i, problems)).filter((f): f is Feature => f !== null);
  return { id, title, features };
}

function readKnownGap(raw: unknown, index: number, problems: string[]): KnownGap | null {
  const at = `known_gaps[${String(index)}]`;
  if (!isObj(raw)) {
    problems.push(`${at}: a known gap must be a map`);
    return null;
  }
  problems.push(...unknownKeys(raw, ["id", "what", "features", "status"], at));
  const id = text(raw, "id", at, problems);
  return {
    id,
    what: text(raw, "what", id || at, problems),
    features: strings(raw, "features", id || at, problems),
    status: text(raw, "status", id || at, problems, ""),
  };
}

/** The registry in `source` (its YAML), and what is wrong with its shape: unknown keys (a typo
 *  would otherwise drop a test silently), missing fields, ids out of their area, ids twice. */
export function parseRegistry(source: string): { registry: Registry; problems: string[] } {
  const problems: string[] = [];
  let doc: unknown;
  try {
    doc = parse(source);
  } catch (e) {
    return { registry: { areas: [], knownGaps: [] }, problems: [`not YAML: ${(e as Error).message}`] };
  }
  if (!isObj(doc)) return { registry: { areas: [], knownGaps: [] }, problems: ["the registry is a map with `areas` and `known_gaps`"] };
  problems.push(...unknownKeys(doc, ["areas", "known_gaps"], "the registry"));
  const areas = (Array.isArray(doc.areas) ? (doc.areas as unknown[]) : []).map((a, i) => readArea(a, i, problems)).filter((a): a is Area => a !== null);
  const knownGaps = (Array.isArray(doc.known_gaps) ? (doc.known_gaps as unknown[]) : [])
    .map((g, i) => readKnownGap(g, i, problems))
    .filter((g): g is KnownGap => g !== null);
  const seen = new Set<string>();
  for (const id of [...areas.map((a) => `area ${a.id}`), ...features({ areas, knownGaps }).map((f) => f.id), ...knownGaps.map((g) => `gap ${g.id}`)]) {
    if (seen.has(id)) problems.push(`${id}: listed twice`);
    seen.add(id);
  }
  return { registry: { areas, knownGaps }, problems };
}

/** Every feature of the registry, area after area. */
export const features = (r: Registry): Feature[] => r.areas.flatMap((a) => a.features);
