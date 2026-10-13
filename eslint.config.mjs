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
