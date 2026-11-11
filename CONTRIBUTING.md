# Contributing

Thanks for helping. This page says how to set up the launcher for development, what a change must
pass, and how to send it. By contributing you agree that your contribution is licensed under the
[Apache License 2.0](LICENSE), like the rest of the project (its section 5); there is no separate
agreement to sign. Please follow the [code of conduct](CODE_OF_CONDUCT.md).

Found a security problem? Don't open an issue: see [SECURITY.md](SECURITY.md).

## Set up

You need:

- Node 22.12 or a later 22, 24, 25, or 26 and later (`engines` in `package.json`:
  `^22.12.0 || ^24.0.0 || ^25.0.0 || >=26.0.0`); `.nvmrc` says 22, the one CI and the release
  check use (`pnpm release-check` stops on another major), and the major `@types/node` stays on
  (Dependabot ignores later ones in `.github/dependabot.yml`: move both with `.nvmrc`); and pnpm
  (the version in `packageManager`; `corepack enable` installs it);
- Rust stable (the crate's floor is `rust-version` in `src-tauri/Cargo.toml`);
- the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS. On Windows that is
  **Visual Studio 2022 Build Tools** with the _Desktop development with C++_ workload; without it
  the build stops at `linker 'link.exe' not found`. On Linux, the WebKitGTK packages
  `.github/actions/apt-deps/action.yml` installs.

```bash
pnpm install
pnpm tauri dev          # the app, with hot reload
pnpm dev:web            # the UI alone in a browser, on sample data: http://127.0.0.1:3207/?mock
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'   # local installers, unsigned
```

`pnpm tauri dev` starts the app's own dev server (`pnpm dev`, port 3206, `devUrl` in
`src-tauri/tauri.conf.json`). It serves the app's window, not browsers:

- A release has no server at all: the app embeds the static export (`out/`) and serves it through
  Tauri's own protocol, so nothing listens on a port for the UI.
- Both dev servers listen on 127.0.0.1 only, never on the network, and Next refuses its dev
  bundles and endpoints to any other origin than `localhost` and that address.
- A browser that opens the app's dev server gets a notice instead of the app; the dev mock is in
  the browser preview's server alone (`pnpm dev:web`, built with `NEXT_PUBLIC_DEV_MOCK=1`, whose
  `?mock` answers the Tauri commands with sample data).

What it can't promise: that only the app's window reads the dev server. A page can't prove to a
server that it runs inside Tauri, and Tauri sends nothing a dev server could check (no header or
token: on the desktop the window loads `devUrl` directly), and a static export has no Next proxy
(middleware) to check one with. Any program on the same machine can read its pages and bundles,
the source included, as it can read the checkout itself. And Next checks the origin of a request
for its dev bundles, not the `Host` it names, so a website in a browser on this machine that
rebinds its own name to 127.0.0.1 could still read them while the dev server runs.

A plain `pnpm tauri build` also writes the signed updater files, which needs the release signing
key. Debug builds keep the sign-in session and the server secrets in files under `~/.cyberctf/`
instead of the OS keychain.

`.gitattributes` keeps every text file LF (Windows scripts CRLF) whatever `core.autocrlf` says; a
checkout made before it may need `git add --renormalize .` once. The tests that syntax-check the
Linux and macOS install scripts with `sh -n` skip that part where no `sh` is on `PATH` (Windows
without Git's `usr\bin`); their other assertions still run.

## Run the tests

| What                                                              | Command                 |
| ----------------------------------------------------------------- | ----------------------- |
| Unit tests: the frontend's, the scripts' and the Rust ones        | `pnpm test`             |
| The UI in Chromium against the dev mock (Playwright, `dev:web`)   | `pnpm test:web`         |
| The real app through its embedded WebDriver (Docker, a VM)        | `pnpm test:desktop`     |
| Real labs on Docker and local VMs (more tiers: `tests/README.md`) | `pnpm test:integration` |
| What CI runs, one verdict per step                                | `pnpm check`            |
| Everything, the desktop end-to-end too: the gate before a release | `pnpm release-check`    |

[tests/README.md](tests/README.md) says what each tier proves and needs, how to run it, every knob,
and where a new test goes.
Every runner script takes `--help`.

## The gates

A pull request must pass what CI runs. `pnpm check` runs all of it on your machine, one
verdict per step ([docs/testing/release-check.md](docs/testing/release-check.md) lists them):
versions, the docs' links, formatting, types, ESLint, the build, the quality ratchet, the unit
tests with the coverage floors, the committed bindings, clippy for the other platforms, `cargo
deny`, `pnpm audit`, shellcheck, actionlint, the actions' pins, the Store listings' JSON and
Playwright against the dev mock. Of the checks CI runs on a pull request, only the Terraform
templates' validation is left to CI; the Docker labs end to end (`labs.yml`) and the packaged
app's smoke test (`package-smoke.yml`) run in CI too. A step whose tool isn't installed, or a
runner step that skipped some of its own steps, reads `skip`, not `ok`. Before you push:

```bash
pnpm format                            # Prettier (TypeScript, CSS, Markdown) and rustfmt
pnpm check                             # what CI runs (pnpm release-check adds the native
                                       # end-to-end, which needs a desktop session, Docker and a
                                       # hypervisor: run it when you can)
```

- **The quality ratchet** (`pnpm quality`, `quality-baseline.json`): a count of lint warnings,
  duplication or dead code may go down, never up.
- **Other platforms**: code behind another OS's `#[cfg]` is compiled only on that OS; CI's matrix
  lints and tests it there. `scripts/check/other-platforms.sh` runs clippy for Linux and Windows
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
