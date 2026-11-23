import { fileURLToPath } from "node:url";
import { configDefaults, defineConfig } from "vitest/config";

/**
 * The test runners' own end-to-end tests: each runs a runner (run.sh, run.mjs, the audit) for real
 * in child processes, with real timers and caps, so it takes seconds per case and slows down
 * with the host's load. They are opt-in: `pnpm test:harness` (vitest.harness.config.mts).
 */
export const HARNESS = [
  "tests/e2e/desktop/runner/run.test.ts",
  "tests/e2e/desktop/support/owned-cli.test.ts",
  "tests/e2e/desktop/support/host.test.ts",
  "tests/integration/run.test.ts",
  "tests/audit/src/kinds.test.ts",
];

// Unit tests for the launcher's frontend logic (stores, pure helpers), the repository's scripts and
// the test runners under tests/. Node environment by default; component tests opt into jsdom with a `// @vitest-environment jsdom` docblock.
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
    include: ["src/**/*.test.ts", "src/**/*.test.tsx", "scripts/**/*.test.ts", "tests/**/*.test.ts"],
    exclude: [...configDefaults.exclude, ...HARNESS],
    setupFiles: ["src/test/setup.ts"],
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/**/*.test.{ts,tsx}", "src/test/**", "src/messages/**", "src/app/**", "src/lib/dev-mock/**"],
      reporter: ["text", "text-summary", "json-summary"],
      // The floor CI enforces: 1 point under what main reaches (at e2a52fd: lines 98.52,
      // functions 96.95, branches 94.99, statements 97.78), so it can only go up. Raise it as
      // tests are added; the Web coverage job summary prints the gap. Keep one `name: number`
      // per line: that workflow reads them from this file.
      thresholds: {
        lines: 97.5,
        functions: 95.9,
        branches: 93.9,
        statements: 96.7,
      },
    },
  },
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
});
