# Scripts

The repo's own tooling: gates and suites run on a developer machine, and the checks CI runs.
Each runner takes `--help`, which prints its header (what it does, its knobs).

| Script                     | Run with                           | What it does                                                                                                        |
| -------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `release-check.sh`         | `pnpm release-check`               | the gate before a release: CI's pull-request checks but the Terraform templates', plus the native e2e, one per step |
| `integration-tests.sh`     | `pnpm test:integration`            | the integration tests: real labs by tier (`local`, `aws`, `server`)                                                 |
| `quality/`                 | `pnpm quality`                     | the ratchet in `quality-baseline.json`: clippy, ESLint warnings, duplication, dead code, unused crates              |
| `check-other-platforms.sh` | `scripts/check-other-platforms.sh` | clippy for Linux and Windows from any machine: the code behind another OS's `#[cfg]`, before CI's matrix            |
| `check-tauri-versions.ts`  | `pnpm check:tauri`                 | the Tauri JS plugins and Rust crates on matching versions                                                           |
| `check-doc-links.ts`       | `pnpm check:docs`                  | every relative link and `#anchor` in the repository's Markdown resolves                                             |
| `third-party.ts`           | `pnpm notices`, `pnpm sbom`        | THIRD_PARTY_NOTICES (cargo-about + the npm licences) and the CycloneDX SBOMs a release ships                        |
| `lib/suite.sh`             | (sourced)                          | what every suite runner shares: `ONLY` / `SKIP` / `STRICT`, verdicts, logged steps, Node from `.nvmrc`              |
| `lib/cli.ts`               | (imported)                         | what the TypeScript runners share: `--help` prints the header comment, as `suite_help` does                         |

`e2e/desktop/run.sh` (the desktop end-to-end) is a suite runner too, kept beside its specs.
[`docs/testing/`](../docs/testing/README.md) says what each layer proves.

## Writing a runner

Source `lib/suite.sh`, call `suite_help "$0" "$@"` first, run each step with `step` (in the open)
or `logged` plus `record` (into a log, with a time limit), and end with `suite_end "<title>"`, which
prints the verdicts and sets the exit status. Keep the header comment current: it is the
runner's `--help`. Scripts run under bash 3.2 (macOS) as well as Linux's, with
`set -uo pipefail`, and pass shellcheck (CI's `lint-infra.yml`).
