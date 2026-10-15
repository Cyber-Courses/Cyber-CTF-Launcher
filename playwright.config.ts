import { defineConfig, devices } from "@playwright/test";

// UI end-to-end tests: the frontend in a plain browser, with the Tauri commands answered by the
// dev mock (src/lib/dev-mock/, `?mock` in the URL). The mock is development only (a production
// build never loads it), so the tests run against `next dev`, not the static export.
const PORT = Number(process.env.E2E_PORT ?? 3210);
const CI = !!process.env.CI;

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: CI,
  retries: CI ? 1 : 0,
  // `next dev` compiles each route on first request: a few workers keep it responsive.
  workers: CI ? 2 : 3,
  timeout: 60_000,
  expect: { timeout: 15_000 },
  reporter: CI ? [["github"], ["html", { open: "never" }], ["list"]] : [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: `http://localhost:${PORT}`,
    viewport: { width: 1280, height: 800 },
    locale: "en-US",
    timezoneId: "Europe/Paris",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 } } }],
  webServer: {
    command: `pnpm exec next dev -p ${PORT}`,
    url: `http://localhost:${PORT}/?mock`,
    reuseExistingServer: !CI,
    timeout: 180_000,
    stdout: "ignore",
    stderr: "pipe",
  },
});
