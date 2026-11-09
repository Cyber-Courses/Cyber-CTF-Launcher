# Testing

Each kind of test lives where its tool expects it, and proves what the one below cannot. A change
is tested at the lowest level that can show it; the higher ones run before a release.

| Kind                | What it drives                                                                      | Where                                                          | Run                         | In CI                                                        |
| ------------------- | ----------------------------------------------------------------------------------- | -------------------------------------------------------------- | --------------------------- | ------------------------------------------------------------ |
| Unit, frontend      | components, hooks and scripts; the Tauri commands answered by a fake                | beside the code: `src/**/*.test.ts(x)`, `scripts/**/*.test.ts` | `pnpm test`                 | `ci.yml`, `web-coverage.yml` (floors in `vitest.config.mts`) |
| Unit, Rust          | the backend, every external tool scripted (`src-tauri/src/platform/exec/fake.rs`)   | beside the code: `#[cfg(test)]` in `src-tauri/src`             | `cargo test` (`src-tauri/`) | `ci.yml`, `coverage.yml`                                     |
| Integration         | real labs through the launcher's Rust on real Docker, VMs, clouds and servers       | `src-tauri/src/integration_tests/` (opt-in, `#[ignore]`)       | `pnpm test:integration`     | `labs.yml` (the Docker labs)                                 |
| End-to-end, web     | the real frontend in Chromium, the commands answered by the dev mock (Playwright)   | `e2e/web/`                                                     | `pnpm e2e`                  | `e2e.yml`                                                    |
| End-to-end, desktop | the real app binary through its embedded WebDriver, on a real Docker and hypervisor | `e2e/desktop/specs/` ([desktop-e2e.md](desktop-e2e.md))        | `pnpm e2e:desktop`          | `native-e2e.yml` (manual, Linux)                             |
| Packaging           | the built bundle installed and launched; the Windows installer's edge cases         | `package-smoke.yml`; `packaging/windows/smoke-test.ps1`        | see each                    | `package-smoke.yml`                                          |

`pnpm release-check` runs everything CI runs plus the desktop end-to-end, one verdict per step
([release-check.md](release-check.md)). [integration.md](integration.md) lists the integration
scenarios and what each guards against; `infra/test-servers/` makes throwaway Proxmox and ESXi
hosts for their server tier.

## Platforms

Every platform the launcher ships for is tested in CI, on the runners `release.yml` builds it on.

| Platform             | Runner             | Unit (Rust) | Installer smoke | Integration (Docker labs, attack box) |
| -------------------- | ------------------ | ----------- | --------------- | ------------------------------------- |
| Linux x64            | `ubuntu-22.04`     | yes         | yes (`.deb`)    | yes (`ubuntu-latest`)                 |
| Linux Arm            | `ubuntu-22.04-arm` | yes         | yes (`.deb`)    | yes (`ubuntu-24.04-arm`, native box)  |
| macOS, Apple Silicon | `macos-latest`     | yes         | yes (`.app`)    | here (`pnpm test:integration`)        |
| macOS, Intel         | `macos-15-intel`   | yes         | yes (`.app`)    | no runner with Docker or a hypervisor |
| Windows x64          | `windows-latest`   | yes         | yes (NSIS)      | no Linux containers on the runner     |
| Windows Arm          | `windows-11-arm`   | yes         | yes (NSIS)      | no Linux containers on the runner     |

What differs by platform is decided in code a unit test covers on every platform (the box's CPU:
`attack_machine/place.rs`; the tools' paths and shells: `platform/`). A real lab on an Intel Mac
or on Windows (Docker Desktop, Hyper-V, VirtualBox) still needs a machine of that kind: run
`pnpm test:integration` there before a release that changes how labs run, and
`packaging/windows/smoke-test.ps1` for the installer's edge cases.

## Where a new test goes

A failure found in the field goes to the lowest level that can reproduce it:

1. A unit test: Rust with the tool scripted (`platform/exec/fake.rs`), or a component test.
2. A dev-mock flag plus a Playwright spec (`src/lib/dev-mock/scenario.ts` lists the flags), for
   what the UI does with an answer.
3. A desktop end-to-end spec, when the real app and engine are the point.
4. An integration test, when only a real tool, hypervisor, cloud or server shows it: an opt-in
   test (`#[ignore = "needs …"]`) in `integration_tests/`, a `scenario` line in
   `scripts/integration-tests.sh`, and a row in [integration.md](integration.md) saying what it
   guards against.

## Conventions

- A test's name says what it proves (`a_change_on_aws_waits_for_a_signed_in_account`), and its
  checks say what they saw.
- A test that starts something stops it, whatever happened, and checks nothing is left (no
  container, VM, cloud instance or network), so a failed run never costs a later one.
- Integration tests read their knobs from `CYBERCTF_TEST_*` and return early without them;
  `cargo test` never runs them. The suite runners (`scripts/lib/suite.sh`) give one verdict per
  step, keep going after a failure, and take `ONLY`, `SKIP` and `STRICT`.
- Logs: `.integration-tests/`, `e2e/desktop/.state/logs/`, and Playwright's report.
