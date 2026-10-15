import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

// Rules at "warn" are counted by the quality ratchet (`pnpm quality`, quality-baseline.json): a
// count may go down, never up. Rules at "error" fail outright.
const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // A promise nobody awaits or catches fails silently (an unhandled rejection): flag it. `void`
  // marks one left to run on purpose, its failure handled inside. Type-aware, so it reads the
  // project's types (the lint takes a few seconds longer). Tests await what they check.
  {
    files: ["src/**/*.{ts,tsx}", "scripts/**/*.ts"],
    ignores: ["src/**/*.test.{ts,tsx}", "src/test/**"],
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname } },
    rules: { "@typescript-eslint/no-floating-promises": ["error", { ignoreVoid: true }] },
  },
  { rules: { "@typescript-eslint/no-explicit-any": "warn" } },
  // Size limits (engineering standard): functions at most 60 lines, components at most 200 and
  // other modules at most 400 (blank lines and comments not counted, JSX counted). Tests are long
  // by nature (one `describe` holds a file's cases) and not limited.
  {
    files: ["src/**/*.{ts,tsx}", "scripts/**/*.ts"],
    ignores: ["src/**/*.test.{ts,tsx}", "src/test/**"],
    rules: {
      "max-lines": ["warn", { max: 400, skipBlankLines: true, skipComments: true }],
      "max-lines-per-function": ["warn", { max: 60, skipBlankLines: true, skipComments: true }],
    },
  },
  {
    files: ["src/**/*.tsx"],
    ignores: ["src/**/*.test.tsx"],
    rules: { "max-lines": ["warn", { max: 200, skipBlankLines: true, skipComments: true }] },
  },
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
