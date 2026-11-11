# Testing

Each kind of test lives where its tool expects it, and proves what the one below cannot. A change
is tested at the lowest level that can show it; the higher ones run before a release. The unit
tests sit beside the code they test; this folder holds the tiers that drive the launcher from
outside: the integration runner, the desktop and the web end-to-end.

```text
tests/
  integration/run.sh     the integration runner: the opt-in scenarios of src-tauri/src/integration_tests/, by tier
  desktop/run.sh         the desktop end-to-end on Linux and macOS (run.ps1: Windows)
  desktop/specs/         one spec a file, `<spec>.mjs`
  desktop/lib/           what the specs share (support.mjs), the smoke's launchers
  web/                   the Playwright specs against the dev mock, `<area>.spec.ts`, and their fixtures
```

## The tiers

| Tier               | What it drives                                                                                               | Where                                                                                                | Needs                                                               | Run                                        | In CI                                          |
| ------------------ | ------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------ | ---------------------------------------------- |
| Unit               | components, hooks and scripts; the backend, every external tool scripted (`platform/exec/fake.rs`)           | beside the code: `src/**/*.test.ts(x)`, `scripts/**/*.test.ts`, `tests/**/*.test.ts`, `#[cfg(test)]` | Node and Rust                                                       | `pnpm test`                                | `ci.yml`, `web-coverage.yml`, `coverage.yml`   |
| Web end-to-end     | the real frontend in Chromium, the commands answered by the dev mock (Playwright)                            | `tests/web/`                                                                                         | Playwright's Chromium (`pnpm exec playwright install chromium`)     | `pnpm test:web`                            | `e2e.yml`                                      |
| Desktop end-to-end | the real app binary through its embedded WebDriver, every feature, on a real Docker, hypervisor and server   | `tests/desktop/` ([desktop-e2e.md](../docs/testing/desktop-e2e.md))                                  | a desktop session; Docker, a hypervisor and a server for some specs | `pnpm test:desktop` (`run.ps1` on Windows) | `native-e2e.yml` (manual, Linux); the test rig |
| Integration        | real labs through the launcher's Rust on real Docker, VMs, clouds and servers                                | `src-tauri/src/integration_tests/` (opt-in, `#[ignore]`), run by `tests/integration/run.sh`          | Docker, Vagrant and a hypervisor; an AWS account, a server by tier  | `pnpm test:integration`                    | `labs.yml` (the Docker labs)                   |
| Packaging          | the built bundle installed and launched (the MSI silently, then removed); the Windows installer's edge cases | `package-smoke.yml`; `packaging/windows/smoke-test.ps1`                                              | the platform's installer                                            | see each                                   | `package-smoke.yml`                            |

`pnpm check` runs every check CI runs on a pull request's code (`ci.yml`, `web-coverage.yml`,
`coverage.yml`, `e2e.yml`, `lint-infra.yml`) but the Terraform templates' validation, one verdict
per step; `pnpm release-check` adds the desktop end-to-end
([release-check.md](../docs/testing/release-check.md)). [integration.md](../docs/testing/integration.md)
lists the integration scenarios and what each guards against; `infra/test-servers/` makes
throwaway Proxmox and ESXi hosts for their server tier.

## Platforms

Every platform the launcher ships for is tested in CI, on the runners `release.yml` builds it on.

| Platform             | Runner             | Unit (Rust) | Installer smoke | Integration (Docker labs, attack box) |
| -------------------- | ------------------ | ----------- | --------------- | ------------------------------------- |
| Linux x64            | `ubuntu-22.04`     | yes         | yes (`.deb`)    | yes (`ubuntu-latest`)                 |
| Linux Arm            | `ubuntu-22.04-arm` | yes         | yes (`.deb`)    | yes (`ubuntu-24.04-arm`, native box)  |
| macOS, Apple Silicon | `macos-latest`     | yes         | yes (`.app`)    | here (`pnpm test:integration`)        |
| macOS, Intel         | `macos-15-intel`   | yes         | yes (`.app`)    | no runner with Docker or a hypervisor |
| Windows x64          | `windows-latest`   | yes         | yes (NSIS, MSI) | no Linux containers on the runner     |
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
   `tests/integration/run.sh`, and a row in [integration.md](../docs/testing/integration.md) saying what
   it guards against.

## Conventions

- A test's name says what it proves (`a_change_on_aws_waits_for_a_signed_in_account`), and its
  checks say what they saw.
- A test that starts something stops it, whatever happened, and checks nothing is left (no
  container, VM, cloud instance or network), so a failed run never costs a later one.
- Integration tests read their knobs from `CYBERCTF_TEST_*` and return early without them;
  `cargo test` never runs them. The suite runners (`scripts/lib/suite.sh`) give one verdict per
  step, keep going after a failure, and take `ONLY`, `SKIP` and `STRICT`.
- Logs: `tests/integration/.state/logs/`, `tests/desktop/.state/logs/`, and Playwright's report.
