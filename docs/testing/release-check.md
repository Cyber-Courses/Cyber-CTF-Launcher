# The release check

One command runs every layer of tests the launcher has, including the one CI cannot run, and
prints one verdict per step:

```sh
scripts/release/release-check.sh                     # or: pnpm release-check
SKIP=e2e-native scripts/release/release-check.sh     # what CI runs, without the native layer: pnpm check
```

It keeps going after a failure, so a single run lists everything that is wrong. `ONLY="…"` and
`SKIP="…"` take step names (the runners it runs as steps, `other-platforms` and `e2e-native`,
don't see them). A step reads `skip` when its tool isn't installed, and so does a runner step that
skipped some of its own steps (a missing cross target, Docker not running): its own summary above
says which. A release is ready when every step is `ok` and the manual checks at
the end of this page have been done for what the release changes.

## What each step proves

| Step              | Command                                   | What it catches                                                                                                                                                     |
| ----------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `prereqs`         | tool presence                             | the machine can run the gate at all (Node 22, the major `.nvmrc` names: another stops it, though `engines` allows 24 and later; pnpm, cargo, cargo-machete, Docker) |
| `install`         | `pnpm install --frozen-lockfile`          | the lockfile and `package.json` agree                                                                                                                               |
| `versions`        | `pnpm check:tauri`                        | Tauri JS plugins and Rust crates drifted apart (fails `tauri build` otherwise)                                                                                      |
| `docs`            | `pnpm check:docs`                         | a relative link or `#anchor` in the Markdown that doesn't resolve                                                                                                   |
| `features`        | `pnpm check:features`                     | a test the feature registry names that doesn't exist, a command, screen, scenario or desktop case no feature claims, a stale `tests/FEATURES.md`                    |
| `format`          | prettier, `cargo fmt --check`             | formatting                                                                                                                                                          |
| `types`           | `pnpm typecheck`                          | TypeScript, including the generated command bindings                                                                                                                |
| `lint`            | `pnpm lint`                               | ESLint                                                                                                                                                              |
| `build`           | `pnpm build`                              | the static export the app ships doesn't build                                                                                                                       |
| `quality`         | `pnpm quality`                            | the ratchet in `quality-baseline.json`: clippy, ESLint warnings, duplication, dead code, commented-out code, unused crates (`cargo machete`)                        |
| `unit-web`        | `vitest run --coverage`                   | the frontend's unit and component tests, with the coverage floors in `vitest.config.mts`                                                                            |
| `unit-rust`       | `cargo test`                              | the Rust tests that need no real tool (external tools are scripted through `platform/exec/fake.rs`), the module conventions among them (`testing/source_scan.rs`)   |
| `bindings`        | `git status` of `src/lib/tauri/generated` | the TypeScript command types the Rust tests rewrote differ from those committed                                                                                     |
| `docs-rust`       | `cargo doc --document-private-items`      | an intra-doc link that resolves to nothing, or a stray HTML tag in a doc comment, as `ci.yml` checks on each platform                                               |
| `other-platforms` | `scripts/check/other-platforms.sh`        | clippy and rustdoc for Linux and Windows, CI's matrix: the code behind another OS's `#[cfg]`; skipped without `rustup`                                              |
| `coverage-rust`   | `cargo llvm-cov`                          | the Rust line floor `coverage.yml` enforces; skipped when `cargo-llvm-cov` is not installed                                                                         |
| `deny`            | `cargo deny check`                        | a Rust dependency with an advisory, a licence `src-tauri/deny.toml` doesn't allow, or another source; skipped when `cargo-deny` is not installed                    |
| `audit`           | `pnpm audit --prod`                       | a known vulnerability in a shipped npm dependency                                                                                                                   |
| `shellcheck`      | `scripts/check/infra.sh shellcheck`       | a warning in the repository's shell scripts, as `lint-infra.yml` runs it; skipped when not installed                                                                |
| `actionlint`      | `actionlint`                              | a mistake in the workflows, as `lint-infra.yml` runs it; skipped when not installed                                                                                 |
| `pins`            | `scripts/check/infra.sh pins`             | an action not pinned to a full commit SHA (a tag can be moved to new code)                                                                                          |
| `store-listings`  | `.github/scripts/check-store-listings.py` | a message catalogue or Store listing that doesn't parse, a listing over Partner Center's limits or naming a missing image; skipped without `python3`                |
| `e2e-mock`        | `pnpm test:web`                           | the real frontend in Chromium, every screen and locale, with Tauri commands answered by the dev mock (`?mock`): flows, failure states, coded errors, a11y, i18n     |
| `e2e-native`      | `tests/desktop/run.sh`                    | the real app binary, driven through its embedded WebDriver server, against a real Docker engine and hypervisor: what no mock can prove                              |

Playwright starts the browser preview (`pnpm dev:web`) on port 3210, or reuses one already there;
the preview builds into its own folder (`.next-web`), so it runs beside `pnpm tauri dev` and the
native runner's dev server, which never serve the mock.

Every step but the last is what CI runs on a pull request (`ci.yml`, `web-coverage.yml`,
`coverage.yml`, `e2e.yml`, and `lint-infra.yml` when the pull request touches the files it checks),
so on a green PR they mostly confirm the local checkout matches. The last one is the reason to run the gate. CI also validates the Terraform
templates (`lint-infra.yml`), which needs Terraform and its providers' registry: the gate leaves it
to CI.

## The native layer

`tests/desktop/run.sh` drives the real app binary through its embedded WebDriver server;
[desktop-e2e.md](desktop-e2e.md) has its scenarios, knobs and why an app from `cargo build` needs the dev server.

## What stays manual

Not in the gate, run when the release touches them:

- `packaging/windows/smoke-test.ps1`: the NSIS installer on Windows (paths with spaces, second
  instance, corrupt settings, read-only data folder, uninstall keeps data).
- The integration tests, `tests/integration/run.sh` ([integration.md](integration.md)): real labs on
  Docker, local VMs, an AWS account and servers (`infra/test-servers/` makes throwaway ones), with
  the attack box on each form. Run it when the release touches running labs.
- The other ignored Rust tests that need real tools: `real_flag_labs_the_flag_contract_holds_on_the_runner` (the flag
  contract, with the backend), `fake_clis` (cloud reaper and hung CLI, `--test-threads=1`),
  `finder_path_finds_tools` (the Finder PATH, `--test-threads=1`),
  `the_real_aws_account_test_confirms_the_launch_permission` (an AWS account signed in with the
  AWS CLI), `real_docker_machine_page_stop_removes_the_labs_volumes_and_networks` and
  `real_docker_clean_up_keeps_the_players_image` (Docker), `proxmox_live_test` (a Proxmox host,
  `CYBERCTF_TEST_PVE_*`), `prints_this_machines_report` (prints the machine report),
  `real_download_is_placed_and_runs` (a downloaded OVF Tool), `utm_is_opened_and_answers_before_a_vm_starts`
  (UTM) and `queries_the_api_anonymously` (the network).
- `labs.yml` and `package-smoke.yml` run in CI, not in the gate: the three Docker labs end to end
  (on each push to `main`, on pull requests that touch the backend, and nightly), and the built
  bundle installed and launched on all three OSes (on pull requests that touch the app, or by
  hand).
- The in-app updater against a real release feed, and sign-in against the real backend.

## Adding coverage

A failure found in the field goes to the lowest layer that can reproduce it: a Rust test with
a scripted tool, a component test, a mock flag plus a Playwright spec (`src/lib/dev-mock/scenario.ts`
lists the flags), and a native scenario only when the real engine is the point.
