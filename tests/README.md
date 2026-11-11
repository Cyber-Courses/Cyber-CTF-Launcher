# Testing

Each kind of test lives where its tool expects it, and proves what the one below cannot. A change
is tested at the lowest level that can show it; the higher ones run before a release. The unit
tests sit beside the code they test; this folder holds the tiers that drive the launcher from
outside: the integration runner, the desktop and the web end-to-end.

```text
tests/
  integration/run.sh     the integration runner: the opt-in scenarios of src-tauri/src/integration_tests/, by tier
  desktop/run.mjs        the desktop end-to-end's runner: the specs, their needs, caps and groups, the results
  desktop/run.sh         its entry point on Linux and macOS (run.ps1: Windows): the app built or unpacked, then run.mjs
  desktop/cases.tsv      the desktop cases, in the runner's order: id, spec, form, knobs, groups
  desktop/specs/         one spec a file, `<spec>.mjs`, its needs declared on its first lines
  desktop/lib/           what the specs share (support.mjs), what a run owns (owned.mjs), the plan (plan.mjs:
                         the cases, the needs, the groups, the verdicts a need not met gives)
  web/                   the Playwright specs against the dev mock (`pnpm dev:web`), `<area>.spec.ts`, and their fixtures
  audit/run.sh           the leftover audit: a snapshot of the machine before and after a run, their diff
                         (snapshot.ps1: Windows; snapshot-server.sh: Proxmox, ESXi); leftovers.md is its contract
  features.yml           the feature registry: every feature the user sees, and the tests that check it per tier
  FEATURES.md            made from it (`pnpm features`): how well each feature is checked, the biggest gaps first
  features-rig.json      the test rig's matrix as FEATURES.md reads it (`pnpm features --matrix …`)
  resilience.md          what a player does that breaks a lab, what the app must do, and which test holds it
```

## The tiers

| Tier               | What it drives                                                                                               | Where                                                                                                | Needs                                                               | Run                                        | In CI                                          |
| ------------------ | ------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------ | ---------------------------------------------- |
| Unit               | components, hooks and scripts; the backend, every external tool scripted (`platform/exec/fake.rs`)           | beside the code: `src/**/*.test.ts(x)`, `scripts/**/*.test.ts`, `tests/**/*.test.ts`, `#[cfg(test)]` | Node and Rust                                                       | `pnpm test`                                | `ci.yml`, `web-coverage.yml`, `coverage.yml`   |
| Web end-to-end     | the real frontend in Chromium, the commands answered by the dev mock (Playwright)                            | `tests/web/`                                                                                         | Playwright's Chromium (`pnpm exec playwright install chromium`)     | `pnpm test:web`                            | `e2e.yml`; `release.yml` before a build        |
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

| Knob                                                                                   | Read by                                                                       | What                                                                                                                                                                              | Was                                          |
| -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------- |
| `CYBERCTF_TEST_INTEGRATION_TIERS`                                                      | the integration runner                                                        | `local`, `aws`, `server` (default `local`)                                                                                                                                        | `TIERS`\*                                    |
| `CYBERCTF_TEST_INTEGRATION_LOGS`                                                       | the integration runner                                                        | where each scenario's log goes (default `tests/integration/.state/logs`)                                                                                                          | `LOGS`\*                                     |
| `CYBERCTF_TEST_INTEGRATION_TIMEOUT`                                                    | the integration runner                                                        | seconds a scenario may take (480; the `attack-box-*` ones 600), past which it is killed                                                                                           |                                              |
| `CYBERCTF_TEST_ISOLOOM_DIR`                                                            | the integration runner                                                        | an Isoloom checkout, for its example labs (default `../isoloom`)                                                                                                                  | `ISOLOOM`\*                                  |
| `CYBERCTF_TEST_VM_PROVIDER`                                                            | the VM scenarios, the desktop's VM specs                                      | the hypervisor: `qemu` (the runner's default), `libvirt`, `virtualbox`, `hyperv`, `utm`…                                                                                          | the runner's `VM_PROVIDER`\*                 |
| `CYBERCTF_TEST_DOCKER_VM_PROVIDER`                                                     | the integration runner                                                        | the one for a container lab in a VM (default `utm` on macOS, else the VM provider)                                                                                                | `DOCKER_VM_PROVIDER`                         |
| `CYBERCTF_TEST_VM_INPUTS`                                                              | `vm-lab`, `per-machine`, `replace-vm`                                         | a VM lab's launch values, `NAME=value,…`                                                                                                                                          |                                              |
| `CYBERCTF_TEST_AWS_REGION`                                                             | the AWS tier, the AWS account test                                            | the region (default `eu-west-3`)                                                                                                                                                  | the runner's `AWS_REGION`                    |
| `CYBERCTF_TEST_CLOUD_REAL_IP`                                                          | the AWS tier (the runner sets it)                                             | `1`: a cloud lab's firewall opens to this machine's real public address (unit tests use a documentation one)                                                                      | `CYBERCTF_TEST_REAL_PUBLIC_IP`               |
| `CYBERCTF_TEST_BOX_LAB`, `_FORM`, `_TARGET`                                            | `attack-box-*`                                                                | the lab, the form (`docker`, `docker-vm`, `vm`, `cloud-docker`, `cloud-vm`), the machine the box reaches (default `web`)                                                          |                                              |
| `CYBERCTF_TEST_BOX_SHARED_FOLDER`                                                      | `attack-box-*`                                                                | a folder shared with the box, seen both ways (unset: off)                                                                                                                         | `CYBERCTF_TEST_SHARED_FOLDER`\*              |
| `CYBERCTF_TEST_BOX_IMAGE`                                                              | `docker-lab-*`                                                                | the attack box's image (default Kali's)                                                                                                                                           | `CYBERCTF_TEST_ATTACK_BOX`                   |
| `CYBERCTF_TEST_HELLO_LAB`, `CYBERCTF_TEST_SEGMENTED_LAB`                               | `docker-lab-hello-stack`, `docker-lab-segmented`                              | Isoloom's `hello-stack` and `segmented` examples                                                                                                                                  |                                              |
| `CYBERCTF_TEST_INVOICE_LAB`                                                            | `docker_lab::supplier_portal_runs_end_to_end` (`labs.yml`), the flag contract | the invoice-portal-api lab                                                                                                                                                        | `CYBERCTF_TEST_LAB`                          |
| `CYBERCTF_TEST_DOCKER_VM_LAB`, `CYBERCTF_TEST_VM_LAB`, `CYBERCTF_TEST_PER_MACHINE_LAB` | `docker-vm-lab`, `vm-lab`, `per-machine`                                      | each scenario's lab                                                                                                                                                               |                                              |
| `CYBERCTF_TEST_REPLACE_LAB`, `_FORM`                                                   | `replace-*`                                                                   | the lab and its form (`docker`, `docker-vm`, `vm`)                                                                                                                                |                                              |
| `CYBERCTF_TEST_RESILIENCE_LAB`, `CYBERCTF_TEST_RESILIENCE_VM_LAB`                      | `resilience-docker-*`, `resilience-vm-*`                                      | the container lab and the VM lab a player's disruptions are tried on (the runner: Isoloom's `hello-stack`, `solo-web`)                                                            |                                              |
| `CYBERCTF_TEST_RESILIENCE_MACHINE`                                                     | `resilience-*`                                                                | the machine removed, stopped or powered off (default `web`)                                                                                                                       |                                              |
| `CYBERCTF_TEST_RESILIENCE_THROWAWAY`                                                   | `resilience-docker-daemon-stopped`, `resilience-vm-killed-provisioning`       | `1` on a throwaway test machine: the scenarios that stop more than their own lab skip without it                                                                                  |                                              |
| `CYBERCTF_TEST_RESILIENCE_DOCKER_STOP`, `_DOCKER_START`                                | `resilience-docker-daemon-stopped`                                            | the commands that stop and start Docker on this OS (`sudo systemctl stop docker`…), run by `sh -c` (PowerShell on Windows)                                                        |                                              |
| `CYBERCTF_TEST_RESILIENCE_VM_KILL`                                                     | `resilience-vm-killed-provisioning`                                           | the hypervisor's own hard power-off of a VM, `{id}` its id in Vagrant's record (`virsh destroy {id}`, `VBoxManage controlvm {id} poweroff`…)                                      |                                              |
| `CYBERCTF_TEST_LOCAL_LAB`                                                              | `launch-commands`, the desktop specs                                          | a lab folder to import (the desktop's default: the fixture checkout)                                                                                                              |                                              |
| `CYBERCTF_TEST_CORPUS_LABS`                                                            | `corpus-docker`                                                               | your own lab folders, comma-separated                                                                                                                                             | `CYBERCTF_TEST_DOCKER_LABS`                  |
| `CYBERCTF_TEST_PVE_LAB`, `_ENDPOINT`, `_TOKEN`, `_NODE`, `_BRIDGE`, `_JUMP`            | `server-lab-proxmox`                                                          | the lab, and the Proxmox host (`_JUMP` when the lab bridge is internal to the node)                                                                                               |                                              |
| `CYBERCTF_TEST_PVE_RUNNER_HOME`                                                        | `server-lab-proxmox`                                                          | a folder whose `ssh/id_ed25519` the node lets in                                                                                                                                  | `CYBERCTF_TEST_RUNNER_HOME`\*                |
| `CYBERCTF_TEST_PVE_HOST`, `_PASSWORD`, `_TOKEN_ID`, `_TOKEN_SECRET`, `_STORAGE`        | `proxmox_live_test`, the desktop's server file                                | a Proxmox host by address and account                                                                                                                                             |                                              |
| `CYBERCTF_TEST_PVE_SSH_KEY`                                                            | `proxmox_live_test`                                                           | a key the node lets in                                                                                                                                                            | `CYBERCTF_TEST_SSH_KEY`\*                    |
| `CYBERCTF_TEST_ESXI_LAB`, `_HOST`, `_PASSWORD`, `_DATASTORE`, `_NETWORK`, `_USER`      | `server-lab-esxi`, the desktop's server file                                  | the lab, and the ESXi host                                                                                                                                                        |                                              |
| `CYBERCTF_TEST_FLAG_BACKEND`, `_WEB`                                                   | the flag contract (`flag_contract`)                                           | CyberBackend and cyberctf-web checkouts                                                                                                                                           | `CYBERCTF_TEST_BACKEND`, `CYBERCTF_TEST_WEB` |
| `CYBERCTF_TEST_SETUP_PATHS`, `_HYPERVISORS`, `_LOGS`, `_LAB`, `_REPORT`, `_CHECK_ONLY` | the guided setup (`integration_tests::setup`)                                 | its module documentation says each; `_CHECK_ONLY=1` detects and plans only, safe on any machine                                                                                   |                                              |
| `CYBERCTF_TEST_DOCKER_IMAGE`                                                           | the real Docker workload test (`machine/workloads.rs`)                        | the image it runs (default `python:3.11-alpine`)                                                                                                                                  | `CYBERCTF_TEST_IMAGE`                        |
| `CYBERCTF_TEST_SCREEN_VNC`                                                             | the real VNC bridge test (`screen/bridge.rs`)                                 | a VNC server with no password, `host:port` on loopback                                                                                                                            | `CYBERCTF_TEST_VNC`                          |
| `CYBERCTF_TEST_DESKTOP_APP`                                                            | the desktop runner, the specs                                                 | the app (`--app`; default: the wdio debug build, built by the runner)                                                                                                             | `APP_BIN`                                    |
| `CYBERCTF_TEST_DESKTOP_TIMEOUT`                                                        | the desktop runner, the specs                                                 | seconds a spec may take (300; `machine` 480), past which it is killed; the specs scale their waits to it ([desktop-e2e.md](../docs/testing/desktop-e2e.md#needs-caps-and-groups)) | `CASE_TIMEOUT`                               |
| `CYBERCTF_TEST_DESKTOP_LOGS`                                                           | the desktop runner                                                            | where each spec's log goes (default `tests/desktop/.state/logs`)                                                                                                                  |                                              |
| `CYBERCTF_TEST_TEMP_KEEP`                                                              | `pnpm check:temp-leaks`                                                       | keep the unit tests' temp folder at the end, its path printed, to look into what they left                                                                                        |                                              |
| `CYBERCTF_TEST_DESKTOP_KEEP`                                                           | the desktop runner                                                            | leave the fixture lab running and the run's folder in place at the end                                                                                                            | `KEEP`                                       |
| `CYBERCTF_TEST_ACCOUNT_SESSION`                                                        | the desktop runner                                                            | a signed-in session (a `dev-session.json`), placed in the run's folder, where its app reads it whatever the build                                                                 | `CYBERCTF_TEST_SESSION`                      |
| `CYBERCTF_TEST_CATALOGUE_LAB`, `CYBERCTF_TEST_CATALOGUE_VM_LAB`                        | the signed-in specs, `lab-form-vm` and `screens`                              | the catalogue container lab (default `Supplier portal API`), a catalogue VM lab                                                                                                   |                                              |
| `CYBERCTF_TEST_SERVER_ENV`, `CYBERCTF_TEST_SERVER_LAB`                                 | `server-lab`                                                                  | a server's settings file (the `CYBERCTF_TEST_PVE_*` or `CYBERCTF_TEST_ESXI_*` above), the catalogue lab to start there                                                            |                                              |
| `CYBERCTF_TEST_LAB_FORM`                                                               | `lab-form` (the runner sets it)                                               | `container`, `docker-vm`, `vm`, `per-machine`                                                                                                                                     | `CYBERCTF_TEST_FORM`                         |
| `CYBERCTF_TEST_SCAN_LAB`                                                               | `vm-scan` (the runner sets it on a Mac)                                       | the id of the lab the running-labs scan must find                                                                                                                                 | `CYBERCTF_EXPECT_LAB`                        |
| `CYBERCTF_TEST_BOX_DESKTOP`                                                            | `screens`                                                                     | `1`: also the attack box's desktop and its Screen (about 1 GB more)                                                                                                               | `CYBERCTF_TEST_ATTACK_DESKTOP`               |
| `CYBERCTF_TEST_CLEANUP_TOOL`                                                           | `cleanup`                                                                     | a tool Settings lists as installed by Cyber CTF, to remove                                                                                                                        | `CYBERCTF_TEST_REMOVE_TOOL`                  |
| `CYBERCTF_TEST_WEB_PORT`                                                               | Playwright                                                                    | the port of the browser preview (`pnpm dev:web`) it starts or reuses (default 3210)                                                                                               | `E2E_PORT`                                   |

\* The old name is still read, as the workspace's test rig sets it: remove it once the rig sets
the new one. `TAURI_WEBDRIVER_PORT` (the embedded WebDriver's port, 4445), `CYBERBACKEND_URL`
(the backend `offline-start`'s relay forwards to) and `CYBERCTF_WDIO_ROOT` (the run's folder,
[below](#a-desktop-runs-own-folder)) are the app's own settings, which the desktop runner and specs
pass on.

## A desktop run's own folder

The desktop end-to-end runs safely on a machine someone uses, a developer's or a player's: it
never reads or changes their labs, settings, session or secrets, and it removes only what it made.

- **Its folders.** Each run makes a fresh folder (`cyberctf-e2e.*` under `TMPDIR`; on Windows
  `cyberctf-e2e-*` under `%TEMP%`) and starts the app with it as `CYBERCTF_WDIO_ROOT`. A `wdio`
  build (never a release) then keeps all of its own there (`src-tauri/src/platform/test_root.rs`),
  the same on every OS: `app/` holds what Tauri resolves as the app's data, config and local data
  folders, with `caches/` and `logs/` (Tauri's `appDirectoriesOverride`, so neither `HOME`, the
  XDG folders nor `APPDATA` are touched), and on Linux and Windows the webviews' data; `home/`
  holds what the app keeps in the home folder, the session, the server secrets and the default
  shared folder, in files whatever the build (a release build's keychain is never used). On
  macOS the webviews' storage is WebKit's to place: they use a data store of their own, the test
  runs', never the user's. The run's app registers no `cyberctf://` handler with the system.
  `fail-signedout` gets a folder of its own (`signed-out/`, no session in it). The tools' own
  homes (Docker's contexts, Vagrant's boxes, SSH keys) stay the user's: the labs run on them.
- **What it removes.** Every clean-up acts on what the run's records say it made
  (`tests/desktop/lib/owned.mjs`, held by `tests/desktop/owned.test.ts`), never on a name: a
  player's own copy of the fixture or catalogue lab has the same Compose project name
  (`cyberctf-<id>`). A Compose project is the run's when its lab is installed in the run's folder
  and every Compose file it was started from lies there; the runners take down only those, and a
  case that imports the fixture lab skips while a copy of it the run didn't start runs. A spec
  that finds its lab already running stops it only when the run started it (`stopLeftover`). An
  image or a box is the run's when it wasn't on the machine when the run started (`records/`,
  written first): `cleanup` confirms Clean up only when every download it would remove is the
  run's, and otherwise names the others and leaves them all. The apps stopped between specs are
  the ones the run launched (`records/apps/<pid>`) and their deploy workers, never another copy
  of the same binary.
- **Afterwards.** The apps' logs (the app's own and its labs' deploy logs, every `*.log` under
  `app/`, the signed-out run's too; never `home/`, and a line naming a password, a token or a key
  left out) are copied beside the specs' logs (`tests/desktop/.state/logs/app/`), whatever the
  verdicts. Then the folder is removed when the run ends, interrupted or not (the run's labs
  taken down first), unless `CYBERCTF_TEST_DESKTOP_KEEP` is set: then the fixture lab stays up
  and the folder stays, its path in the summary. On macOS the webviews' data store is the run's
  own (its identifier made from the run's folder), so nothing a run's pages stored shows in the
  next run.

## Results

A caller that drives these tiers (the workspace's test rig) reads their results, not their
screen. Each entry point appends **one JSON line per check** to a file it is given, and lists its
cases with the knobs each reads, so a renamed case or knob fails the caller's own check, not a
night's run:

| Entry point                                                   | Results                                                                              | Listing                                                    |
| ------------------------------------------------------------- | ------------------------------------------------------------------------------------ | ---------------------------------------------------------- |
| `tests/integration/run.sh`                                    | `--results <file>`, a line a scenario                                                | `--list`                                                   |
| `tests/desktop/run.sh`, `run.ps1` (`run.mjs`)                 | `--results <file>` (`-Results`), a line a spec and a line a check (`<spec>/<check>`) | `--list` (`-List`), from `desktop/cases.tsv` and the specs |
| the guided setup (`integration_tests::setup`, a `cargo test`) | `CYBERCTF_TEST_RESULTS=<file>`, a line an item                                       | in the integration runner's `--list` (tier `bare-machine`) |
| the leftover audit (`tests/audit/run.sh diff`)                | `--results <file>`, a line a kind of resource ([leftovers.md](leftovers.md))         | `--list`                                                   |

```json
{ "suite": "integration", "case": "vm-lab", "provider": "qemu", "form": "vm", "result": "pass", "reason": "", "seconds": 612, "log": "…/vm-lab.log" }
```

- `suite`: `integration`, `desktop`, `setup` or `audit`; `case`: the scenario, spec, item or kind of
  resource, as `--list` names it (a desktop check: `<spec>/<check>`, not counted again in its spec's verdict).
- `provider`: the hypervisor, cloud (`aws`) or server (`proxmox`, `esxi`) it ran on; `form`: the
  form its lab ran in (`docker`, `docker-vm`, `vm`, `per-machine`, `cloud-docker`, `cloud-vm`, or
  the desktop's `container`); each empty where none applies.
- `result`: `pass`, `fail` or `skip`; `reason`: why it failed or was skipped (a setup item's:
  `<status>: <note>`, its status `installed`, `present`, `failed`, `skipped` or `again`, the last
  a pass that ends for a `reboot` or a `relogin`); `seconds`; `log`: its log.

The file is appended to, never truncated, so one file can gather a whole run. A listing line:

```json
{"suite":"integration","case":"vm-lab","tier":"local","form":"vm","test":"integration_tests::vm_lab::a_vm_lab_runs_end_to_end_through_the_runner","knobs":["CYBERCTF_TEST_VM_LAB","CYBERCTF_TEST_VM_PROVIDER","CYBERCTF_TEST_VM_INPUTS"]}
{"suite":"desktop","case":"lab-form-vm","spec":"lab-form","form":"vm","knobs":["CYBERCTF_TEST_LAB_FORM","CYBERCTF_TEST_CATALOGUE_LAB","CYBERCTF_TEST_CATALOGUE_VM_LAB","CYBERCTF_TEST_VM_PROVIDER"],"groups":["vm"],"needs":["vm-start"],"provides":[]}
```

A desktop case's `groups` are the shards a caller may run as parallel jobs (`--group <name>`),
its `needs` what the runner meets before it runs, and `provides` what its pass proves for the
others ([desktop-e2e.md](../docs/testing/desktop-e2e.md#needs-caps-and-groups)). A case skipped
for a need not met carries the need's failure as its `reason`.

Unit tests hold both to the code: `tests/integration/run.test.ts` (each listed test exists, every
knob a scenario reads is listed), `tests/desktop/run.test.ts` (`cases.tsv` against the specs, the
plan, and the runner on fake specs and a fake app), `scripts/lib/suite.test.ts` and
`integration_tests/support/results.rs` (the records).

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

A unit test removes what it writes in the temp folder: a Rust test takes its folder from
`testing::temp_dir` (removed when it drops, a failed test's too), a TypeScript one removes its own
in `afterEach` or `onTestFinished`. `pnpm check:temp-leaks` runs the unit tests with a temp folder
of their own and fails on anything left in it; CI and `pnpm check` run them through it.

## The feature registry

[`features.yml`](features.yml) lists what the launcher does for its user, area by area, and the
tests that check each feature, tier by tier; [`FEATURES.md`](FEATURES.md) is made from it and says
how well each feature is checked (Strong, Partial, Unchecked) and where the biggest gaps are.
`pnpm check:features` (in `pnpm check` and CI) keeps it honest: every test it names exists, every
command (`src-tauri/src/commands.rs`), screen, window and Settings section belongs to a feature, so
does every integration scenario and desktop case, and FEATURES.md is up to date. The tooling is
`scripts/features/`.

A feature:

```yaml
- id: lab-run.vm # <area>.<kebab-case name>
  what: Start and stop a VM lab on this machine's hypervisor.
  platforms: [linux, macos, windows] # omitted: all three
  providers: [libvirt, qemu, virtualbox, hyperv, utm]
  host: true # it depends on the real machine: only a real-host tier (desktop, integration, setup, package) proves it
  commands: [lab_launch] # the Tauri commands it is the home of (each command has one)
  pages: [tab:labs] # route:/<path>, tab:<screen>, settings:<section>
  tests:
    unit:
      - src-tauri/src/runtime/lab.rs > a_vm_lab_starts # a Rust test by its name
      - src/features/labs/lab-store.test.ts # a whole test file
    web:
      - "vm-labs.spec.ts > on this machine: up, pause" # words of its title; quoted when they hold ": "
    desktop: [lab-form-vm, lab-form-vm/running]
    integration: [vm-lab]
    setup: [selftest:vm]
  rig: [vm, vm@qemu] # rig/MATRIX.md cells that run it: a column, `@<setup>` for the rows readied that way
  gaps: # per tier, what its tests leave out: the tier shows ◐ with this note
    integration: never run on Windows
```

A reference names a test exactly, and the check finds it:

| Tier          | A reference                                                                                            |
| ------------- | ------------------------------------------------------------------------------------------------------ |
| `unit`        | a test file (`src/…/x.test.tsx`, a Rust file with tests), or `<file> > <Rust fn or title words>`       |
| `web`         | `<spec>.spec.ts > <words of the test's title>`                                                         |
| `desktop`     | a case of `tests/desktop/run.sh --list`, or `<case>/<check>` (a spec's `r.check`/`r.step` id)          |
| `integration` | a scenario of `tests/integration/run.sh --list`, or `<Rust file> > <fn>` for an opt-in test            |
| `package`     | the packaging check's file (`.github/workflows/package-smoke.yml`, `packaging/windows/smoke-test.ps1`) |
| `setup`       | an item of `integration_tests::setup` (`docker`, `selftest:vm`, `plugin:<name>`…)                      |

**Registering a new test.** Add it to the feature it checks, in its tier, then `pnpm features`
and commit both files. A new integration scenario or desktop case fails the check until a feature
claims it, as a new command or screen does until a feature is its home. A test that checks only
part of a feature (it renders the page, it runs on Docker only) gets a `gaps` note for its tier:
read the test, not its name. A feature that depends on the machine (`host: true`) is Strong only
with a real-host tier that covers it and, when it names rig cells, one of them passed. A new
feature, or a gap the registry should track, goes in the same file (`known_gaps` holds the ones
being built or planned). After a rig run, `pnpm features --matrix ../rig/MATRIX.md` (from the
workspace's submodule) refreshes the rig columns.

## Conventions

- A test's name says what it proves (`a_change_on_aws_waits_for_a_signed_in_account`), and its
  checks say what they saw.
- A test that starts something stops it, whatever happened, and checks nothing is left (no
  container, VM, cloud instance or network), so a failed run never costs a later one.
- Integration tests read their knobs (`CYBERCTF_TEST_<AREA>_<WHAT>`, [above](#knobs)) and return
  early without them; `cargo test` never runs them. The suite runners (`scripts/lib/suite.sh`)
  give one verdict per step, keep going after a failure, and take `ONLY`, `SKIP` and `STRICT`.
- Logs: `tests/integration/.state/logs/`, `tests/desktop/.state/logs/`, and Playwright's report.
