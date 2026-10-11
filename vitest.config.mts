import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// Unit tests for the launcher's frontend logic (stores, pure helpers). Node environment by
// default; component tests opt into jsdom with a `// @vitest-environment jsdom` docblock.
// `@/` resolves to src/, matching tsconfig paths.
//
// Coverage is measured over ALL of src (every file matched by `include`, loaded or not), so the
// number reflects untested code too. Route shells, translation catalogs and the browser dev mock
// are excluded: they hold no logic worth unit testing.
export default defineConfig({
  test: {
    environment: "node",
    // Generous per-test budget: under coverage instrumentation a cold dynamic import of a module
    // graph can take seconds on slow CI runners. Tests themselves stay fast.
    testTimeout: 20_000,
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    setupFiles: ["src/test/setup.ts"],
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/**/*.test.{ts,tsx}", "src/test/**", "src/messages/**", "src/app/**", "src/lib/dev-mock.ts"],
      reporter: ["text", "text-summary", "json-summary"],
      // The floor CI enforces: 2 points under the level reached, so it can only go up. Raise
      // it as tests are added.
      thresholds: {
        lines: 96,
        functions: 95,
        branches: 93,
        statements: 95,
      },
    },
  },
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
});
