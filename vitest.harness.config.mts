import { defineConfig } from "vitest/config";
import base, { HARNESS } from "./vitest.config.mts";

// The test runners' end-to-end tests (see HARNESS in vitest.config.mts): `pnpm test:harness`.
// They spawn real processes, so each gets a minute; they measure no coverage of src.
export default defineConfig({
  ...base,
  test: {
    environment: "node",
    setupFiles: base.test?.setupFiles,
    include: HARNESS,
    testTimeout: 60_000,
  },
});
