import { defineConfig, globalIgnores } from "eslint/config";
import nextVitals from "eslint-config-next/core-web-vitals";
import nextTs from "eslint-config-next/typescript";

const eslintConfig = defineConfig([
  ...nextVitals,
  ...nextTs,
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
