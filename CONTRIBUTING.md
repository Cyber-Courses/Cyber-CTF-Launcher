# Contributing

Thanks for helping. This page says how to set up the launcher for development, what a change must
pass, and how to send it. By contributing you agree that your contribution is licensed under the
[Apache License 2.0](LICENSE), like the rest of the project (its section 5); there is no separate
agreement to sign. Please follow the [code of conduct](CODE_OF_CONDUCT.md).

Found a security problem? Don't open an issue: see [SECURITY.md](SECURITY.md).

## Set up

You need:

- Node 22, 24, 25 or later (`engines` in `package.json`; `.nvmrc` says 22) and pnpm (the version
  in `packageManager`; `corepack enable` installs it);
- Rust stable (the crate's floor is `rust-version` in `src-tauri/Cargo.toml`);
- the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS. On Windows that is
  **Visual Studio 2022 Build Tools** with the _Desktop development with C++_ workload; without it
  the build stops at `linker 'link.exe' not found`. On Linux, the WebKitGTK packages
  `.github/actions/apt-deps/action.yml` installs.

```bash
pnpm install
pnpm tauri dev          # the app, with hot reload
pnpm dev                # the UI alone in a browser, on sample data: http://localhost:1437/?mock
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'   # local installers, unsigned
```

A plain `pnpm tauri build` also writes the signed updater files, which needs the release signing
key. Debug builds keep the sign-in session and the server secrets in files under `~/.cyberctf/`
instead of the OS keychain.

`.gitattributes` keeps every text file LF (Windows scripts CRLF) whatever `core.autocrlf` says; a
checkout made before it may need `git add --renormalize .` once. The tests that syntax-check the
Linux and macOS install scripts with `sh -n` skip that part where no `sh` is on `PATH` (Windows
without Git's `usr\bin`); their other assertions still run.

## Run the tests

| What                                                           | Command                                           |
| -------------------------------------------------------------- | ------------------------------------------------- |
| Frontend unit and component tests                              | `pnpm test`                                       |
| Rust unit tests                                                | `cargo test --manifest-path src-tauri/Cargo.toml` |
| The UI in Chromium against the dev mock (Playwright)           | `pnpm e2e`                                        |
| The real app through its embedded WebDriver (Docker, a VM)     | `pnpm e2e:desktop`                                |
| Real labs on Docker and local VMs (`TIERS="local aws server"`) | `pnpm test:integration`                           |
| Everything, one verdict per step: the gate before a release    | `pnpm release-check`                              |

[docs/testing/](docs/testing/README.md) says what each layer proves and where a new test goes.
Every runner script takes `--help`.

## The gates

A pull request must pass what CI runs. Run them before you push:

```bash
pnpm format     # Prettier (TypeScript, CSS, Markdown) and rustfmt
pnpm check      # versions, formatting, types, the quality ratchet (clippy, ESLint, duplication,
                # dead code), and the unit tests (vitest, cargo test)
pnpm lint       # ESLint
cargo deny --manifest-path src-tauri/Cargo.toml check   # advisories, licences, sources
pnpm audit --prod
```

- **The quality ratchet** (`pnpm quality`, `quality-baseline.json`): a count of lint warnings,
  duplication or dead code may go down, never up.
- **Other platforms**: code behind another OS's `#[cfg]` is compiled only on that OS; CI's matrix
  lints and tests it there. `scripts/check-other-platforms.sh` runs clippy for Linux and Windows
  from any machine first (no tests: those need the platform).
- **Generated types**: `cargo test` rewrites the TypeScript types of the Tauri commands
  (`src/lib/tauri/generated/`) from the Rust structs ([ts-rs](https://github.com/Aleph-Alpha/ts-rs));
  commit them with the change, or CI fails. A command taking or returning a new struct or enum
  derives `ts_rs::TS` and is listed in `src-tauri/src/testing/ts_bindings.rs`.
- **Dependencies**: a new crate must be under a licence `src-tauri/deny.toml` allows (Apache-2.0
  compatible). Tauri's JS packages and Rust crates move together (`pnpm check:tauri`).
- **Workflows**: every action is pinned to a full commit SHA with its version in a comment
  (`uses: owner/repo@<sha> # vX.Y.Z`); `lint-infra.yml` runs actionlint and shellcheck.

## Commits and pull requests

- One change per commit. The subject is a plain sentence saying what changes, in the present
  tense, without a type prefix or a final period ("Fail a launch whose chosen container engine
  has no context here"). The body, when there is one, says why.
- Add a line to `CHANGELOG.md` under **Unreleased** for anything a user or an administrator
  would notice ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/) sections).
- Update the docs the change makes wrong ([docs/README.md](docs/README.md) lists them).
- Open the pull request against `main` and fill in its template. CI runs on it; a maintainer
  reviews it ([.github/CODEOWNERS](.github/CODEOWNERS)).

## Where things are

[docs/architecture.md](docs/architecture.md) maps the code: the Next.js UI (`src/`), the Rust core
(`src-tauri/`), and how labs run through Isoloom and the Isothrum runner.
[docs/design-system.md](docs/design-system.md) is the UI's design system, and
[docs/release-process.md](docs/release-process.md) how a release is made.
