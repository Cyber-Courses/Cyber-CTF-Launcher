// The quality ratchet's command line (`scripts/quality/main.ts`).

/** What a run of the ratchet was asked to do. */
export type QualityArgs = { update: boolean; force: boolean; list: string | null };

/** Reads the ratchet's arguments: `--update`, `--force` (with `--update` only), `--list [prefix]`
 *  anywhere among them. Anything else is refused, so a typo can't run a plain check that looks
 *  like the one asked for. */
export function parseArgs(args: readonly string[]): QualityArgs | { error: string } {
  const out: QualityArgs = { update: false, force: false, list: null };
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === "--update") out.update = true;
    else if (a === "--force") out.force = true;
    else if (a === "--list") {
      const next = args.at(i + 1);
      out.list = next !== undefined && !next.startsWith("--") ? next : "";
      if (out.list) i++;
    } else return { error: `unknown argument \`${a}\` (see pnpm quality --help)` };
  }
  if (out.force && !out.update) return { error: "--force goes with --update" };
  return out;
}
