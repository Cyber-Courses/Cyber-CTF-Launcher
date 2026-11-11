import { defineConfig, devices } from "@playwright/test";

// UI end-to-end tests: the frontend in a plain browser, with the Tauri commands answered by the
// dev mock (src/lib/dev-mock/, `?mock` in the URL). Only the browser preview's dev server
// (`pnpm dev:web`, scripts/dev/web.ts) has the mock: not a production build, nor the app's own
// dev server, which shows a browser a notice. So the tests run against `pnpm dev:web`.
const PORT = Number(process.env.CYBERCTF_TEST_WEB_PORT ?? 3210);
const CI = !!process.env.CI;

export default defineConfig({
  testDir: "./tests/web",
  fullyParallel: true,
  forbidOnly: CI,
  // On CI a test gets one retry, for the report's trace, but one that passes only on its retry
  // still fails the run: a flaky test is fixed, not left to rot behind a green check.
  retries: CI ? 1 : 0,
  failOnFlakyTests: CI,
  // `next dev` compiles each route on first request: a few workers keep it responsive.
  workers: CI ? 2 : 3,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  reporter: CI ? [["github"], ["html", { open: "never" }], ["list"]] : [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    viewport: { width: 1280, height: 800 },
    locale: "en-US",
    timezoneId: "Europe/Paris",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } } }],
  webServer: {
    command: `pnpm dev:web --port ${String(PORT)}`,
    url: `http://127.0.0.1:${String(PORT)}/?mock`,
    reuseExistingServer: !CI,
    timeout: 180_000,
    stdout: "ignore",
    stderr: "pipe",
  },
});
