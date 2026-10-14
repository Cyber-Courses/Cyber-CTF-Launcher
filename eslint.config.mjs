import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
  // A promise nobody awaits or catches fails silently (an unhandled rejection): flag it. `void`
  // marks one left to run on purpose, its failure handled inside. Type-aware, so it reads the
  // project's types (the lint takes a few seconds longer). Tests await what they check.
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/**/*.test.{ts,tsx}", "src/test/**"],
    languageOptions: { parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname } },
    rules: { "@typescript-eslint/no-floating-promises": ["error", { ignoreVoid: true }] },
  },
  // Size limits (#150): about 50 lines per function and 400 per file are the targets; these caps
  // (blank lines and comments not counted, JSX counted) stop new code from growing past what the
  // largest components are now. Tests are long by nature and not limited.
  {
    files: ["src/**/*.{ts,tsx}"],
    ignores: ["src/**/*.test.{ts,tsx}", "src/test/**"],
    rules: {
      "max-lines": ["error", { max: 400, skipBlankLines: true, skipComments: true }],
      "max-lines-per-function": ["error", { max: 100, skipBlankLines: true, skipComments: true }],
    },
  },
  // Still over the caps, to split once the fixes in flight on them have landed (#150).
  {
    files: [
      "src/lib/dev-mock.ts",
      "src/components/command-palette.tsx",
      "src/features/cloud/account-row.tsx",
      "src/features/home/home-screen.tsx",
      "src/features/machine/downloads-panel.tsx",
    ],
    rules: { "max-lines": "off", "max-lines-per-function": "off" },
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
