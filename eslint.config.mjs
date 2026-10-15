import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";
import sonarjs from "eslint-plugin-sonarjs";
import tseslint from "typescript-eslint";

// The engineering standard. A rule at "error" fails the lint. A rule at "warn" is one the code
// doesn't meet yet: the quality ratchet (`pnpm quality`, quality-baseline.json) counts it, and its
// count may go down, never up; once it reaches zero the rule goes back to "error" here.

/** Every TypeScript file ESLint checks. */
const CODE = ["src/**/*.{ts,tsx}", "scripts/**/*.ts"];
/** Tests and their helpers. */
const TESTS = ["src/**/*.test.{ts,tsx}", "src/test/**"];
/** Next.js entry points, which must be default exports. */
const ENTRY_POINTS = ["src/app/**/{page,layout,error,global-error,not-found,loading}.tsx"];

/** Rules the shipped code and the tests don't meet yet: none, every area is at zero. A rule added
 *  before the code meets it goes here, at "warn", until its count reaches zero. */
const RATCHETED = [];
/** Rules only the tests don't meet yet: none (as above). */
const RATCHETED_IN_TESTS = [];

/** A rule's level: "warn" while the ratchet counts it, else "error". */
const level = (rule) => (RATCHETED.includes(rule) ? "warn" : "error");
/** `rule` at its level, with `options`. */
const rule = (name, ...options) => [name, [level(name), ...options]];
/** typescript-eslint's strict-type-checked rules at their levels (eslint-config-next registers
 *  the plugin); the ones it turns off stay off. */
const strictTypeChecked = tseslint.configs.strictTypeChecked
  .flatMap((config) => Object.entries(config.rules ?? {}))
  .map(([name, setting]) => {
    const [severity, ...options] = Array.isArray(setting) ? setting : [setting];
    return severity === "off" ? [name, "off"] : rule(name, ...options);
  });

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // Type-aware: the rules read the project's types (the lint takes longer). A promise nobody
  // awaits or catches fails silently; `void` marks one left to run on purpose.
  {
    files: CODE,
    plugins: { sonarjs },
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname } },
    rules: Object.fromEntries([
      ...strictTypeChecked,
      rule("@typescript-eslint/no-floating-promises", { ignoreVoid: true }),
      rule("@typescript-eslint/no-explicit-any"),
      rule("@typescript-eslint/max-params", { max: 5 }),
      rule("sonarjs/no-commented-code"),
    ]),
  },
  { files: CODE, ignores: ENTRY_POINTS, rules: Object.fromEntries([rule("import/no-default-export")]) },
  // Size limits: functions at most 60 lines and cognitive complexity 15, components at most 200
  // lines and other modules at most 400 (blank lines and comments not counted, JSX counted). Tests
  // are long by nature (one `describe` holds a file's cases) and not limited.
  {
    files: CODE,
    ignores: TESTS,
    rules: Object.fromEntries([
      rule("max-lines", { max: 400, skipBlankLines: true, skipComments: true }),
      rule("max-lines-per-function", { max: 60, skipBlankLines: true, skipComments: true }),
      rule("sonarjs/cognitive-complexity", 15),
    ]),
  },
  {
    files: ["src/**/*.tsx"],
    ignores: TESTS,
    rules: Object.fromEntries([rule("max-lines", { max: 200, skipBlankLines: true, skipComments: true })]),
  },
  // Configured above for the tests too: only the level changes.
  { files: TESTS, rules: Object.fromEntries(RATCHETED_IN_TESTS.map((name) => [name, "warn"])) },
  // Override default ignores of eslint-config-next.
  globalIgnores([
    // Default ignores of eslint-config-next:
    ".next/**",
    "out/**",
    "build/**",
    "next-env.d.ts",
    // Rust build output (Tauri generates JS there).
    "src-tauri/target/**",
    // The command types ts-rs writes from the Rust structs (`cargo test`), checked by CI as is.
    "src/lib/tauri/generated/**",
  ]),
]);

export default eslintConfig;
