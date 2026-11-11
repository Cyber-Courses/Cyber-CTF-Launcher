// What keeps the feature registry honest against the repository: every test it names exists
// (a scenario or case its runner lists, a spec's check, a test file, a test in it), and
// everything a user can reach maps to a feature (each command and screen), as does every
// integration scenario and desktop case. `pnpm check:features` runs it (main.ts).
import { matchesSetupItem, type Facts } from "./facts.ts";
import { rigRefProblem, type RigSnapshot } from "./matrix.ts";
import { features, type Feature, type Registry, type Tier } from "./registry.ts";

/** A reference's file and, after ` > `, what to find in it (a Rust test's name, a test title's words). */
export function splitRef(ref: string): { file: string; needle: string | null } {
  const at = ref.indexOf(" > ");
  return at < 0 ? { file: ref.trim(), needle: null } : { file: ref.slice(0, at).trim(), needle: ref.slice(at + 3).trim() };
}

const RUST_TEST = /#\[(?:tokio::)?test\b|proptest!/;
const TS_TEST = /\.test\.tsx?$/;

/** Whether `needle` is in `text`: a Rust function by name, anything else as written. */
function holds(file: string, text: string, needle: string): boolean {
  if (file.endsWith(".rs")) return new RegExp(`\\bfn ${needle.replace(/[^\w]/g, "")}\\s*[(<]`).test(text) && /^\w+$/.test(needle);
  return text.includes(needle);
}

/** Why a file reference doesn't resolve, or null. `kind` says what the file must be. */
function fileProblem(ref: string, facts: Facts, kind: "unit" | "rust" | "web" | "any"): string | null {
  const { file, needle } = splitRef(ref);
  const path = kind === "web" ? `tests/web/${file}` : file;
  const text = facts.read(path);
  if (text === null) return `no file ${path}`;
  if (kind === "unit" && !TS_TEST.test(file) && !(file.endsWith(".rs") && RUST_TEST.test(text))) return `${file} is not a unit test file`;
  if (kind === "rust" && !file.endsWith(".rs")) return `${file} is not a Rust file`;
  if (kind === "web" && !file.endsWith(".spec.ts")) return `${file} is not a Playwright spec`;
  if (needle !== null && !holds(file, text, needle)) return `${file} has no ${file.endsWith(".rs") ? `fn ${needle}` : `"${needle}"`}`;
  return null;
}

function desktopProblem(ref: string, facts: Facts): string | null {
  const [id, check] = ref.split("/", 2) as [string, string | undefined];
  const spec = facts.desktopCases.get(id);
  if (spec === undefined) return `no desktop case ${id} (tests/desktop/run.sh --list)`;
  if (check !== undefined && !facts.desktopChecks.get(spec)?.has(check)) return `spec ${spec} has no check ${check}`;
  return null;
}

/** Why the reference `ref` of `tier` doesn't resolve, or null when it does. */
export function refProblem(tier: Tier, ref: string, facts: Facts): string | null {
  switch (tier) {
    case "unit":
      return fileProblem(ref, facts, "unit");
    case "web":
      return fileProblem(ref, facts, "web");
    case "desktop":
      return desktopProblem(ref, facts);
    case "integration":
      if (ref.includes(" > ")) return fileProblem(ref, facts, "rust");
      return facts.scenarios.includes(ref) ? null : `no integration scenario ${ref} (tests/integration/run.sh --list)`;
    case "setup":
      return matchesSetupItem(ref, facts.setupItems) ? null : `no setup item ${ref} (integration_tests/setup.rs: ${facts.setupItems.join(", ")})`;
    case "package":
      return fileProblem(ref, facts, "any");
  }
}

function featureProblems(f: Feature, facts: Facts, rig: RigSnapshot): string[] {
  const out: string[] = [];
  for (const [tier, refs] of Object.entries(f.tests) as [Tier, string[]][]) {
    for (const ref of refs) {
      const why = refProblem(tier, ref, facts);
      if (why) out.push(`${f.id}: ${tier} \`${ref}\`: ${why}`);
    }
  }
  for (const ref of f.rig) {
    const why = rigRefProblem(rig, ref);
    if (why) out.push(`${f.id}: rig \`${ref}\`: ${why}`);
  }
  for (const c of f.commands.filter((c) => !facts.commands.includes(c))) out.push(`${f.id}: no command ${c} (src-tauri/src/commands.rs)`);
  for (const p of f.pages.filter((p) => !facts.pages.includes(p))) out.push(`${f.id}: no page ${p} (known: ${facts.pages.join(", ")})`);
  return out;
}

/** Each of `all` no feature claims (through `claimed`), as a problem; and each claimed twice. */
function unclaimed(all: readonly string[], claimed: Map<string, string[]>, what: string, once: boolean): string[] {
  const out = all.filter((x) => !claimed.has(x)).map((x) => `${what} ${x} belongs to no feature: add it to one in tests/features.yml`);
  if (once) for (const [x, by] of claimed) if (by.length > 1) out.push(`${what} ${x} is the home of one feature, not ${by.join(", ")}`);
  return out;
}

function claims(list: Feature[], of: (f: Feature) => string[]): Map<string, string[]> {
  const m = new Map<string, string[]>();
  for (const f of list) for (const x of new Set(of(f))) m.set(x, [...(m.get(x) ?? []), f.id]);
  return m;
}

/** Every problem of `registry` against `facts`; empty when it is honest. */
export function problems(registry: Registry, facts: Facts, rig: RigSnapshot): string[] {
  const list = features(registry);
  const ids = new Set(list.map((f) => f.id));
  const out = list.flatMap((f) => featureProblems(f, facts, rig));
  for (const g of registry.knownGaps) for (const id of g.features.filter((id) => !ids.has(id))) out.push(`known gap ${g.id}: no feature ${id}`);
  out.push(
    ...unclaimed(
      facts.commands,
      claims(list, (f) => f.commands),
      "command",
      true,
    ),
  );
  out.push(
    ...unclaimed(
      facts.pages,
      claims(list, (f) => f.pages),
      "page",
      false,
    ),
  );
  // The guided setup is one scenario of the runner's list, which its items stand for.
  const scenarios = claims(list, (f) => [...f.tests.integration, ...(f.tests.setup.length ? ["setup"] : [])]);
  out.push(...unclaimed(facts.scenarios, scenarios, "integration scenario", false));
  out.push(
    ...unclaimed(
      [...facts.desktopCases.keys()],
      claims(list, (f) => f.tests.desktop.map((r) => r.split("/")[0])),
      "desktop case",
      false,
    ),
  );
  return out;
}
