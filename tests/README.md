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

## Knobs

Every knob is `CYBERCTF_TEST_<AREA>_<WHAT>`: the area is the scenario, spec or shared resource
that reads it (`BOX` the attack box, `VM` the local hypervisor, `PVE` a Proxmox host, `DESKTOP` the
desktop runner…), the what is the setting. A knob is read from the environment; the desktop
runner also takes them from a `--knobs` file (`KEY=value` lines, read literally). A scenario or
spec whose knob is missing skips, saying which. `ONLY`, `SKIP` and `STRICT` aren't knobs but the
suite runners' selection (`scripts/lib/suite.sh`).

| Knob                                                                                   | Read by                                                                       | What                                                                                                                                     | Was                                          |
| -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| `CYBERCTF_TEST_INTEGRATION_TIERS`                                                      | the integration runner                                                        | `local`, `aws`, `server` (default `local`)                                                                                               | `TIERS`\*                                    |
| `CYBERCTF_TEST_INTEGRATION_LOGS`                                                       | the integration runner                                                        | where each scenario's log goes (default `tests/integration/.state/logs`)                                                                 | `LOGS`\*                                     |
| `CYBERCTF_TEST_ISOLOOM_DIR`                                                            | the integration runner                                                        | an Isoloom checkout, for its example labs (default `../isoloom`)                                                                         | `ISOLOOM`\*                                  |
| `CYBERCTF_TEST_VM_PROVIDER`                                                            | the VM scenarios, the desktop's VM specs                                      | the hypervisor: `qemu` (the runner's default), `libvirt`, `virtualbox`, `hyperv`, `utm`…                                                 | the runner's `VM_PROVIDER`\*                 |
| `CYBERCTF_TEST_DOCKER_VM_PROVIDER`                                                     | the integration runner                                                        | the one for a container lab in a VM (default `utm` on macOS, else the VM provider)                                                       | `DOCKER_VM_PROVIDER`                         |
| `CYBERCTF_TEST_VM_INPUTS`                                                              | `vm-lab`, `per-machine`, `replace-vm`                                         | a VM lab's launch values, `NAME=value,…`                                                                                                 |                                              |
| `CYBERCTF_TEST_AWS_REGION`                                                             | the AWS tier, the AWS account test                                            | the region (default `eu-west-3`)                                                                                                         | the runner's `AWS_REGION`                    |
| `CYBERCTF_TEST_CLOUD_REAL_IP`                                                          | the AWS tier (the runner sets it)                                             | `1`: a cloud lab's firewall opens to this machine's real public address (unit tests use a documentation one)                             | `CYBERCTF_TEST_REAL_PUBLIC_IP`               |
| `CYBERCTF_TEST_BOX_LAB`, `_FORM`, `_TARGET`                                            | `attack-box-*`                                                                | the lab, the form (`docker`, `docker-vm`, `vm`, `cloud-docker`, `cloud-vm`), the machine the box reaches (default `web`)                 |                                              |
| `CYBERCTF_TEST_BOX_SHARED_FOLDER`                                                      | `attack-box-*`                                                                | a folder shared with the box, seen both ways (unset: off)                                                                                | `CYBERCTF_TEST_SHARED_FOLDER`\*              |
| `CYBERCTF_TEST_BOX_IMAGE`                                                              | `docker-lab-*`                                                                | the attack box's image (default Kali's)                                                                                                  | `CYBERCTF_TEST_ATTACK_BOX`                   |
| `CYBERCTF_TEST_HELLO_LAB`, `CYBERCTF_TEST_SEGMENTED_LAB`                               | `docker-lab-hello-stack`, `docker-lab-segmented`                              | Isoloom's `hello-stack` and `segmented` examples                                                                                         |                                              |
| `CYBERCTF_TEST_INVOICE_LAB`                                                            | `docker_lab::supplier_portal_runs_end_to_end` (`labs.yml`), the flag contract | the invoice-portal-api lab                                                                                                               | `CYBERCTF_TEST_LAB`                          |
| `CYBERCTF_TEST_DOCKER_VM_LAB`, `CYBERCTF_TEST_VM_LAB`, `CYBERCTF_TEST_PER_MACHINE_LAB` | `docker-vm-lab`, `vm-lab`, `per-machine`                                      | each scenario's lab                                                                                                                      |                                              |
| `CYBERCTF_TEST_REPLACE_LAB`, `_FORM`                                                   | `replace-*`                                                                   | the lab and its form (`docker`, `docker-vm`, `vm`)                                                                                       |                                              |
| `CYBERCTF_TEST_LOCAL_LAB`                                                              | `launch-commands`, the desktop specs                                          | a lab folder to import (the desktop's default: the fixture checkout)                                                                     |                                              |
| `CYBERCTF_TEST_CORPUS_LABS`                                                            | `corpus-docker`                                                               | your own lab folders, comma-separated                                                                                                    | `CYBERCTF_TEST_DOCKER_LABS`                  |
| `CYBERCTF_TEST_PVE_LAB`, `_ENDPOINT`, `_TOKEN`, `_NODE`, `_BRIDGE`, `_JUMP`            | `server-lab-proxmox`                                                          | the lab, and the Proxmox host (`_JUMP` when the lab bridge is internal to the node)                                                      |                                              |
| `CYBERCTF_TEST_PVE_RUNNER_HOME`                                                        | `server-lab-proxmox`                                                          | a folder whose `ssh/id_ed25519` the node lets in                                                                                         | `CYBERCTF_TEST_RUNNER_HOME`\*                |
| `CYBERCTF_TEST_PVE_HOST`, `_PASSWORD`, `_TOKEN_ID`, `_TOKEN_SECRET`, `_STORAGE`        | `proxmox_live_test`, the desktop's server file                                | a Proxmox host by address and account                                                                                                    |                                              |
| `CYBERCTF_TEST_PVE_SSH_KEY`                                                            | `proxmox_live_test`                                                           | a key the node lets in                                                                                                                   | `CYBERCTF_TEST_SSH_KEY`\*                    |
| `CYBERCTF_TEST_ESXI_LAB`, `_HOST`, `_PASSWORD`, `_DATASTORE`, `_NETWORK`, `_USER`      | `server-lab-esxi`, the desktop's server file                                  | the lab, and the ESXi host                                                                                                               |                                              |
| `CYBERCTF_TEST_FLAG_BACKEND`, `_WEB`                                                   | the flag contract (`flag_contract`)                                           | CyberBackend and cyberctf-web checkouts                                                                                                  | `CYBERCTF_TEST_BACKEND`, `CYBERCTF_TEST_WEB` |
| `CYBERCTF_TEST_SETUP_PATHS`, `_HYPERVISORS`, `_LOGS`, `_LAB`, `_REPORT`                | the guided setup (`integration_tests::setup`)                                 | its module documentation says each                                                                                                       |                                              |
| `CYBERCTF_TEST_DOCKER_IMAGE`                                                           | the real Docker workload test (`machine/workloads.rs`)                        | the image it runs (default `python:3.11-alpine`)                                                                                         | `CYBERCTF_TEST_IMAGE`                        |
| `CYBERCTF_TEST_SCREEN_VNC`                                                             | the real VNC bridge test (`screen/bridge.rs`)                                 | a VNC server with no password, `host:port` on loopback                                                                                   | `CYBERCTF_TEST_VNC`                          |
| `CYBERCTF_TEST_DESKTOP_APP`                                                            | the desktop runner, the specs                                                 | the app (`--app`; default: the wdio debug build, built by the runner)                                                                    | `APP_BIN`                                    |
| `CYBERCTF_TEST_DESKTOP_TIMEOUT`                                                        | the desktop runner                                                            | seconds for a spec with no VM in it (1800; the VM and server specs get 3 hours)                                                          | `CASE_TIMEOUT`                               |
| `CYBERCTF_TEST_DESKTOP_KEEP`                                                           | the desktop runner                                                            | leave the fixture lab running at the end                                                                                                 | `KEEP`                                       |
| `CYBERCTF_TEST_ACCOUNT_SESSION`                                                        | the desktop runner                                                            | a signed-in session (a debug build's `dev-session.json`), placed for the run, the one there put back; a release build reads the keychain | `CYBERCTF_TEST_SESSION`                      |
| `CYBERCTF_TEST_CATALOGUE_LAB`, `CYBERCTF_TEST_CATALOGUE_VM_LAB`                        | the signed-in specs, `lab-form-vm` and `screens`                              | the catalogue container lab (default `Supplier portal API`), a catalogue VM lab                                                          |                                              |
| `CYBERCTF_TEST_SERVER_ENV`, `CYBERCTF_TEST_SERVER_LAB`                                 | `server-lab`                                                                  | a server's settings file (the `CYBERCTF_TEST_PVE_*` or `CYBERCTF_TEST_ESXI_*` above), the catalogue lab to start there                   |                                              |
| `CYBERCTF_TEST_LAB_FORM`                                                               | `lab-form` (the runner sets it)                                               | `container`, `docker-vm`, `vm`, `per-machine`                                                                                            | `CYBERCTF_TEST_FORM`                         |
| `CYBERCTF_TEST_SCAN_LAB`                                                               | `vm-scan` (the runner sets it on a Mac)                                       | the id of the lab the running-labs scan must find                                                                                        | `CYBERCTF_EXPECT_LAB`                        |
| `CYBERCTF_TEST_BOX_DESKTOP`                                                            | `screens`                                                                     | `1`: also the attack box's desktop and its Screen (about 1 GB more)                                                                      | `CYBERCTF_TEST_ATTACK_DESKTOP`               |
| `CYBERCTF_TEST_CLEANUP_TOOL`                                                           | `cleanup`                                                                     | a tool Settings lists as installed by Cyber CTF, to remove                                                                               | `CYBERCTF_TEST_REMOVE_TOOL`                  |
| `CYBERCTF_TEST_WEB_PORT`                                                               | Playwright                                                                    | the port of the dev server it starts or reuses (default 3210)                                                                            | `E2E_PORT`                                   |

\* The old name is still read, as the workspace's test rig sets it: remove it once the rig sets
the new one. `TAURI_WEBDRIVER_PORT` (the embedded WebDriver's port, 4445) and `CYBERBACKEND_URL`
(the backend `offline-start`'s relay forwards to) are the app's own settings, which the desktop
runner and specs pass on.

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
- Integration tests read their knobs (`CYBERCTF_TEST_<AREA>_<WHAT>`, [above](#knobs)) and return
  early without them; `cargo test` never runs them. The suite runners (`scripts/lib/suite.sh`)
  give one verdict per step, keep going after a failure, and take `ONLY`, `SKIP` and `STRICT`.
- Logs: `tests/integration/.state/logs/`, `tests/desktop/.state/logs/`, and Playwright's report.
