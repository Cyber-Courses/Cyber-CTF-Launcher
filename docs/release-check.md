# The release check

One command runs every layer of tests the launcher has, including the one CI cannot run, and
prints one verdict per step:

```sh
dev/release-check.sh          # or: pnpm release-check
```

It keeps going after a failure, so a single run lists everything that is wrong. `ONLY="…"` and
`SKIP="…"` take step names. A release is ready when every step is `ok` and the manual checks at
the end of this page have been done for what the release changes.

## What each step proves

| Step | Command | What it catches |
| --- | --- | --- |
| `prereqs` | tool presence | the machine can run the gate at all (Node from `.nvmrc`, pnpm, cargo, cargo-machete, Docker) |
| `install` | `pnpm install --frozen-lockfile` | the lockfile and `package.json` agree |
| `versions` | `pnpm check:tauri` | Tauri JS plugins and Rust crates drifted apart (fails `tauri build` otherwise) |
| `format` | prettier, `cargo fmt --check` | formatting |
| `types` | `pnpm typecheck` | TypeScript, including the generated command bindings |
| `lint` | `pnpm lint` | ESLint |
| `quality` | `pnpm quality` | the ratchet in `quality-baseline.json`: clippy, ESLint warnings, duplication, dead code, module conventions, unused crates (`cargo machete`) |
| `unit-web` | `vitest run --coverage` | the frontend's unit and component tests, with the coverage floors in `vitest.config.mts` |
| `unit-rust` | `cargo test` | the Rust tests that need no real tool (external tools are scripted through `platform/exec/fake.rs`) |
| `coverage-rust` | `cargo llvm-cov` | the Rust line floor `coverage.yml` enforces; skipped when `cargo-llvm-cov` is not installed |
| `e2e-mock` | `playwright test` | the real frontend in Chromium, every screen and locale, with Tauri commands answered by the dev mock (`?mock`): flows, failure states, coded errors, a11y, i18n |
| `e2e-native` | `tests/native/run-all.sh` | the real app binary, driven through its embedded WebDriver server, against a real Docker engine and hypervisor: what no mock can prove |

Next allows one dev server per checkout (its dev lock), so when one already serves port 3000
(`pnpm tauri dev`, or the native runner's), the `e2e-mock` step runs Playwright against it
instead of starting its own on port 3210.

The first eleven steps are what CI runs on every pull request (`ci.yml`, `web-coverage.yml`,
`coverage.yml`, `e2e.yml`), so on a green PR they mostly confirm the local checkout matches. The
last one is the reason to run the gate.

## The native layer

`tests/native/run-all.sh` builds the app with `--features wdio` (the embedded W3C WebDriver
server, never in a release build), serves the frontend a debug build loads (`tauri.conf.json`'s
`devUrl`, port 3000; a release build embeds `out/` and needs none), checks out the fixture lab
(the Supplier Portal API lab, under `dev/test/server/.state/labs`), and runs the scenarios in an
order that leaves nothing behind:

| Scenario | Proves |
| --- | --- |
| `smoke` | the app boots and its shell renders |
| `fail-offline` | with the backend unreachable (`CYBERBACKEND_URL` dead), the catalogue degrades and the app stays usable |
| `fail-badlab` | a folder that is not a lab is refused without a crash or a phantom running lab |
| `fail-signedout` | signed out (an empty `HOME`, where a debug build keeps its session), the app is usable and asks to sign in |
| `fail-docker` | with a dead `DOCKER_HOST`, the engine shows as not running and no lab starts |
| `launch` | the seeded local lab reaches "running on this machine" on the Overview |
| `restart` | after the app is killed and reopened without the seed, the running-labs scan finds the lab again |
| `stop` | Stop in Machine > Running now stops and clears it |
| `launch-ui` | the real import path (`__wdio_launch_local__`) reaches running |
| `attack-box` | the attack box attaches, gets an address, opens a shell and detaches |
| `kill-mid-deploy` | killed mid-deploy, the app reopens coherently |
| `vm-scan` | on a Mac with UTM: a started VM named `<lab> · <machine>` is matched to the installed lab by the running-labs scan (the fixture VM is created and deleted by the runner) |

Each scenario writes `tests/native/.state/logs/<name>.log`. Knobs: `APP_BIN`,
`TAURI_WEBDRIVER_PORT`, `DEV_URL_PORT`, `CYBERCTF_TEST_LOCAL_LAB`, `CASE_TIMEOUT`, `ONLY`,
`SKIP_BUILD`, `KEEP` (leave the lab running), `STRICT` (a skip fails the run). The scenarios share
`tests/native/lib.mjs`; a new one is a short `.mjs` beside them plus a line in `run-all.sh`.

On Linux the VM scan is skipped: start a libvirt domain named `<lab>_<machine>` (or a Hyper-V VM
`<lab>-<machine>`) yourself and run `functional-vm-scan.mjs` with `CYBERCTF_EXPECT_LAB=<lab id>`.
On Windows, `tests/native/run-embedded.ps1` runs the smoke; the scenarios take `APP_BIN` there too.

Why a debug build needs the dev server: Tauri's `generate_context!` loads `devUrl` in a debug
build and the static export only in a release build, so a debug app started by hand shows a blank
window until `next dev` answers on port 3000. The runner handles it.

Why the scripts force the page visible: the frontend pauses its polls while the document is
hidden (`src/lib/visibility.ts`), and on macOS a window another one covers reports hidden, so a
driven app never learned that its lab came up. `lib.mjs` overrides `document.visibilityState`
and re-applies it on every wait tick (a reload drops it).

## What stays manual

Not in the gate, run when the release touches them:

- `dev/test/windows/smoke.ps1`: the NSIS installer on Windows (paths with spaces, second
  instance, corrupt settings, read-only data folder, uninstall keeps data).
- `dev/test/server/`: a real lab on Proxmox, ESXi and a local VM through `deploy/vagrant`
  (throwaway hosts, see its README).
- The ignored Rust tests that need real tools: `cargo test -- --ignored real_vm_labs`,
  `real_server_labs`, `real_docker_vm_labs`, `real_flag_labs`, `fake_clis` (cloud reaper and hung
  CLI, `--test-threads=1`), `finder_path_finds_tools` (the Finder PATH, `--test-threads=1`).
- `labs.yml` and `package-smoke.yml` run in CI on each push: the three Docker labs end to end, and
  the built bundle installed and launched on all three OSes.
- The in-app updater against a real release feed, and sign-in against the real backend.

## Adding coverage

A failure found in the field goes to the lowest layer that can reproduce it: a Rust test with
a scripted tool, a component test, a mock flag plus a Playwright spec (`src/lib/dev-mock/scenario.ts`
lists the flags), and a native scenario only when the real engine is the point.
