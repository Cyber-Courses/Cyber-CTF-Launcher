import { describe, expect, it } from "vitest";

import { parseArgs } from "./args.ts";

describe("parseArgs", () => {
  it("reads --update, --force and --list wherever they are", () => {
    expect(parseArgs([])).toEqual({ update: false, force: false, list: null });
    expect(parseArgs(["--update", "--force"])).toEqual({ update: true, force: true, list: null });
    // Regression: --list was read only as the first argument.
    expect(parseArgs(["--update", "--list", "rs:machine"])).toEqual({ update: true, force: false, list: "rs:machine" });
    expect(parseArgs(["--list"])).toEqual({ update: false, force: false, list: "" });
    expect(parseArgs(["--list", "--update"])).toEqual({ update: true, force: false, list: "" });
  });

  it("refuses an argument it doesn't know, instead of running a plain check", () => {
    // Regression: a typo (`--updat`) ran a check that looked like the update asked for.
    expect(parseArgs(["--updat"])).toEqual({ error: "unknown argument `--updat` (see pnpm quality --help)" });
    expect(parseArgs(["--force"])).toEqual({ error: "--force goes with --update" });
  });
});
