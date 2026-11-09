# The release check

One command runs every layer of tests the launcher has, including the one CI cannot run, and
prints one verdict per step:

```sh
scripts/release-check.sh          # or: pnpm release-check
```

It keeps going after a failure, so a single run lists everything that is wrong. `ONLY="…"` and
`SKIP="…"` take step names. A release is ready when every step is `ok` and the manual checks at
the end of this page have been done for what the release changes.

## What each step proves

| Step            | Command                          | What it catches                                                                                                                                                 |
| --------------- | -------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `prereqs`       | tool presence                    | the machine can run the gate at all (Node from `.nvmrc`, pnpm, cargo, cargo-machete, Docker)                                                                    |
| `install`       | `pnpm install --frozen-lockfile` | the lockfile and `package.json` agree                                                                                                                           |
| `versions`      | `pnpm check:tauri`               | Tauri JS plugins and Rust crates drifted apart (fails `tauri build` otherwise)                                                                                  |
| `format`        | prettier, `cargo fmt --check`    | formatting                                                                                                                                                      |
| `types`         | `pnpm typecheck`                 | TypeScript, including the generated command bindings                                                                                                            |
| `lint`          | `pnpm lint`                      | ESLint                                                                                                                                                          |
| `quality`       | `pnpm quality`                   | the ratchet in `quality-baseline.json`: clippy, ESLint warnings, duplication, dead code, module conventions, unused crates (`cargo machete`)                    |
| `unit-web`      | `vitest run --coverage`          | the frontend's unit and component tests, with the coverage floors in `vitest.config.mts`                                                                        |
| `unit-rust`     | `cargo test`                     | the Rust tests that need no real tool (external tools are scripted through `platform/exec/fake.rs`)                                                             |
| `coverage-rust` | `cargo llvm-cov`                 | the Rust line floor `coverage.yml` enforces; skipped when `cargo-llvm-cov` is not installed                                                                     |
| `e2e-mock`      | `playwright test`                | the real frontend in Chromium, every screen and locale, with Tauri commands answered by the dev mock (`?mock`): flows, failure states, coded errors, a11y, i18n |
| `e2e-native`    | `e2e/desktop/run.sh`             | the real app binary, driven through its embedded WebDriver server, against a real Docker engine and hypervisor: what no mock can prove                          |

Next allows one dev server per checkout (its dev lock), so when one already serves port 3000
(`pnpm tauri dev`, or the native runner's), the `e2e-mock` step runs Playwright against it
instead of starting its own on port 3210.

The first eleven steps are what CI runs on every pull request (`ci.yml`, `web-coverage.yml`,
`coverage.yml`, `e2e.yml`), so on a green PR they mostly confirm the local checkout matches. The
last one is the reason to run the gate.

## The native layer

`e2e/desktop/run.sh` drives the real app binary through its embedded WebDriver server;
[desktop-e2e.md](desktop-e2e.md) has its scenarios, knobs and why a debug build needs the dev server.

## What stays manual

Not in the gate, run when the release touches them:

- `packaging/windows/smoke-test.ps1`: the NSIS installer on Windows (paths with spaces, second
  instance, corrupt settings, read-only data folder, uninstall keeps data).
- The integration tests, `scripts/integration-tests.sh` ([integration.md](integration.md)): real labs on
  Docker, local VMs, an AWS account and servers (`infra/test-servers/` makes throwaway ones), with
  the attack box on each form. Run it when the release touches running labs.
- The other ignored Rust tests that need real tools: `real_flag_labs` (the flag contract, with
  the backend), `fake_clis` (cloud reaper and hung CLI, `--test-threads=1`),
  `finder_path_finds_tools` (the Finder PATH, `--test-threads=1`).
- `labs.yml` and `package-smoke.yml` run in CI on each push: the three Docker labs end to end, and
  the built bundle installed and launched on all three OSes.
- The in-app updater against a real release feed, and sign-in against the real backend.

## Adding coverage

A failure found in the field goes to the lowest layer that can reproduce it: a Rust test with
a scripted tool, a component test, a mock flag plus a Playwright spec (`src/lib/dev-mock/scenario.ts`
lists the flags), and a native scenario only when the real engine is the point.
