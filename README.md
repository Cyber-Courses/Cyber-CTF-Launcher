# Cyber CTF Desktop

Desktop launcher for Cyber CTF labs, built with [Tauri 2](https://tauri.app). It runs CTF labs on the player's own machine, servers and cloud accounts, using [Isoloom](https://www.isoloom.com) to read each lab and convert it to the right infrastructure files, and the [Isothrum](https://github.com/isoloom/isothrum) runner to run them. This app replaced the earlier Electron launcher in this repository.

## Where labs run

A lab describes itself once as an `isoloom.yml` (its machines, networks, services and checks). The launcher uses Isoloom to read that spec and convert it to each target's native files (Compose, Vagrant, Terraform) under the lab's `.isoloom/` folder, then runs them in one of three modes:

| Mode | What runs the lab |
|------|-------------------|
| **Local** | Docker on the player's machine (Docker Compose), or VMs through Vagrant on any local hypervisor (VirtualBox, VMware Desktop, Hyper-V, Parallels, libvirt, QEMU, UTM). Containers can also run inside a local VM (`docker-vm`). |
| **Server** | The player's own **Proxmox** or **VMware ESXi** server, through Vagrant or Terraform. |
| **Cloud** | The player's own **AWS, Azure, GCP, DigitalOcean, Linode or Oracle Cloud** account, through Terraform with that provider's CLI credentials. |

### Lab engine

Two engines run a lab's generated files, chosen under **Settings > Labs > Lab engine**:

- **Built-in** (the default): the launcher's own lifecycle code, which runs every mode above.
- **Isothrum runner**: the shared Isothrum runner, which executes the versioned run manifest Isoloom writes alongside the generated files. It currently drives local Docker labs, local VM labs, and **Proxmox** and **ESXi** server labs; `docker-vm` and the cloud modes stay on the launcher's own code whichever engine is selected. The developer environment variable `CYBERCTF_RUNNER` (`isothrum` or `launcher`) forces one regardless of the setting.

**Proven vs. generated.** Local labs (Docker and VMs) and server labs (Proxmox and ESXi) are proven by real runs. The cloud modes are **generated and validated** — the Terraform each cloud produces passes its official validator — but have not been run end to end on a paid account, so treat cloud runs as generate-and-validate-proven until exercised with real credentials.

## Architecture

- **UI:** Next.js as a **static export** (`output: "export"`) served by Tauri. There is no Next server, so no API routes, SSR or middleware. The UI only calls typed Rust commands ([`src/lib/tauri/`](src/lib/tauri/), one module per area).
  - [`src/features/`](src/features/): one folder per page with its parts and hooks: `labs` (catalogue, lab page, deployment steps, network diagram), `machine` (Machine page and the setup steps), `servers`, `cloud`, `settings`, `home`, `hosted`, `onboarding`, `account`.
  - [`src/components/ui/`](src/components/ui/): shared building blocks (Button, Panel, RadioRow, Segmented, ChoiceCard, StatusPill, LogConsole…); [`src/components/`](src/components/) holds the app shell.
  - [`src/lib/`](src/lib/): data access ([`tauri/`](src/lib/tauri/)), per-machine settings, formatting ([`format.ts`](src/lib/format.ts)) and small helpers.
- **Core (Rust, [`src-tauri/`](src-tauri/)):** everything privileged. Processes are spawned without a shell, lab ids are validated, and the UI never passes paths or command lines.
  - [`account/`](src-tauri/src/account/): Cyber Auth sign-in (`auth.rs`: authorization code + PKCE through the system browser, loopback redirect on `127.0.0.1:47290-47292/callback`; tokens in the OS keychain, refreshed automatically, never in the webview), CyberBackend GraphQL with the token attached on the Rust side (`api.rs`), and the agent that links the launcher to the website (`agent.rs`, `colocation.rs`).
  - [`labs.rs`](src-tauri/src/labs.rs): `lab_launch` calls `startLab`, then downloads `CyberCTF/*` at the pinned commit (full SHA only; archive paths checked), and starts the lab with `CTF_API_URL` and `CTF_LAUNCH_TOKEN`. The lab's `evidence` service exchanges the token for this player's evidence.
  - [`machine/`](src-tauri/src/machine/): what this machine can run (`system/`: Docker engines, Vagrant, hypervisors, package manager), the setup self-tests (`selftest/`), the workloads/storage the Machine page shows (`workloads/`, `storage.rs`) and one-click dependency installs (`tools/`).
  - [`platform/`](src-tauri/src/platform/): running tools without a shell (`exec/`), files only this user can read (`private_fs.rs`), whole-file writes (`atomic_file.rs`), HTTP clients with timeouts and a download size cap (`http.rs`), and the PATH a GUI launch is missing (`env_path.rs`).
  - [`runtime/`](src-tauri/src/runtime/): lab lifecycle, with logs streamed to the UI over a Tauri channel
    - `lab/`: the launcher's one door to Isoloom (its crates — `isoloom-spec`, `-plan`, `-targets`, `-compose`, `-render` and the run-manifest types `isoloom-manifest` — are used nowhere else): the target's files (Compose, Vagrant, Terraform) are generated from the lab's `isoloom.yml` right before they run, then the launcher's own overrides applied (`overrides.rs`); Isoloom's paths and names (`names.rs`); the variables of Isoloom's Terraform modules from the player's hosts and accounts (`vars.rs`); and each lab's instance numbering (`instances.rs`)
    - `isothrum.rs` + `isothrum/`: the Isothrum runner path (`isothrum-runner`) — when the Isothrum runner is the lab engine, the launcher hands it local Docker labs, local VM labs and Proxmox/ESXi server labs, and it executes the run manifest Isoloom wrote; its runs live under the app data folder's `runs/`, never in the lab folder
    - `launch.rs`, `lifecycle.rs`, `status.rs`: start, park/resume, stop and the auto-stop reaper for cloud labs, and what is running where
    - `docker/`: `docker compose -p cyberctf-<lab>`; stop removes volumes, so every start is a clean lab; status reports machines, interfaces, networks and declared services
    - `vm.rs` + `vm/`: `vagrant up --provider <p>`; stop destroys the VMs
    - `server/`, `proxmox.rs`, `terraform/`: labs on the player's own servers (ESXi, Proxmox) and cloud accounts (AWS, Azure, GCP, DigitalOcean, Linode, OCI); `server_selftest/` checks a server or account before the first lab
    - `providers.rs`: Vagrant providers. Local: VirtualBox, VMware Desktop, Hyper-V, Parallels, libvirt, QEMU, UTM. Remote: VMware ESXi, Proxmox.
    - `exegol.rs`, `attack_box.rs`, `attack_vm.rs` + `attack_vm/`: the attack box (container) or attack VM beside the lab
  - [`deploy_worker/`](src-tauri/src/deploy_worker/): launches, resumes, parks and provisioning runs in a detached worker process (`cyberctf-desktop deploy --job <file>`), so quitting the app never cuts a deploy short. A per-lab file lock keeps the app and the workers from running two operations on one lab at once.
  - [`lan_relay/`](src-tauri/src/lan_relay/): loopback relays that let Vagrant and ovftool reach a LAN ESXi host on macOS.
  - `terminal.rs`: the in-app attack box shell (a PTY streamed to xterm.js); `shared_folder.rs`: the folder shared with the attack box.
  - `cloud.rs`: cloud account sign-in through each provider's CLI; `error.rs`: crate-wide errors.
  - [`app/`](src-tauri/src/app/): the app shell (windows, menu, quitting, the log file, the service endpoints in `config.rs`); [`commands.rs`](src-tauri/src/commands.rs): the commands the webview invokes.

The runtime layer knows nothing about CTFs: a lab is a directory with an `isoloom.yml` (its machines, networks, services and checks). The launcher generates the files for the target it runs on under the lab's `.isoloom/` folder.

Labs live in `<app data>/labs/<lab id>/`.

### Platform notes

A VM lab runs natively only where its boxes exist for the host architecture. On Apple Silicon, ARM boxes run natively on UTM/QEMU (Apple Hypervisor). x86-only boxes such as pfSense run there only under QEMU emulation, which is several times slower; heavy x86 labs should use a remote x86 host (ESXi, Proxmox).

Hyper-V only takes management commands from an administrator (or a member of "Hyper-V Administrators"), and Vagrant's Hyper-V provider refuses to run otherwise. On Windows, Vagrant commands on Hyper-V machines run through a generated, hidden Windows PowerShell script ([`platform/exec/elevate.rs`](src-tauri/src/platform/exec/elevate.rs)); when the launcher itself may not manage Hyper-V, Windows first asks for administrator rights (a UAC prompt, raised for the app's window). Status reads are never elevated, and Vagrant refuses even those without the rights: so the Machine report shows Hyper-V as not ready for an account that may not manage it, and Settings > Hypervisor offers **Allow my account**, which adds the account to "Hyper-V Administrators" (one UAC prompt; it takes effect at the next Windows sign-in).

- Prompts: one per batch of Vagrant commands. A lab's own start (boot, then power off the setup machine), provisioning run, pause or shutdown, resume and stop are one batch each. A VM lab with an attack VM asks once more for the attack VM in each of these, and again for each retry of an attack VM start that hit Vagrant's machine lock. The VM self-test asks twice (boot and run a command, then delete the test VM), three times when it first deletes a test VM left from an earlier run. Started as administrator (or as a Hyper-V administrator), the launcher asks nothing.
- Output streams as usual: the script writes it to a log the launcher follows. The environment the commands need (tokens included) goes through a file in the batch's own temp folder, read and deleted by the script, never on a command line.
- Vagrant's questions: on Windows, Vagrant waits forever for an answer even with no one to give it. The script picks a virtual switch ("Default Switch", else an external one, else the first) unless `ISOLOOM_HYPERV_SWITCH` already names one, and passes it as `ISOLOOM_HYPERV_SWITCH`: a lab's Isoloom Vagrantfile puts each VM's first adapter on it (and its lab networks on private switches `isoloom-<lab>-<network>`), and the launcher's own Vagrantfiles (self-test, attack VM) attach to it too. The attack VM gets no shared folder on Hyper-V (Vagrant would ask for a Windows password to share it over SMB). If Vagrant asks anyway, the batch is stopped with a message saying so.
- Stopping: cancelling an operation, the deploy worker being stopped, or the launcher exiting ends the batch and every process it started; a batch running for more than 6 hours is ended too. Declining the UAC prompt fails the operation with a plain message.

## Roadmap

- [x] System check, Docker/VM runtimes, provider detection
- [x] Login: system browser + PKCE against cyberauth.co, loopback redirect, tokens in the OS keychain
- [x] Lab catalogue from [CyberBackend](https://github.com/Cyber-Courses/CyberBackend); launch = `startLab`, download at the pinned commit, run with the launch token
- [x] Servers (ESXi/Proxmox, credentials in the keychain) and cloud accounts (AWS, Azure, GCP, DigitalOcean, Linode, OCI)
- [x] Attack box on the lab network (Cyber CTF image on Docker Hub, built by the attack-box workflow)
- [x] VM lab provisioning from the lab's `isoloom.yml` (through Isoloom), re-runnable per machine from the lab page
- [x] Signed builds + auto-update (GitHub Releases; the in-app updater installs signed updates from `latest.json`)

## Configuration

Production endpoints are built in ([`app/config.rs`](src-tauri/src/app/config.rs)). Each can be overridden at build or run time, e.g. to test against a local cyber-auth:

| Variable | Default |
|----------|---------|
| `CYBERCTF_AUTH_BASE` | `https://www.cyberauth.co/api/auth` |
| `CYBERBACKEND_URL` | `https://cyberbackend.com` (also the token audience) |
| `CYBERCTF_CLIENT_ID` | `cyberctf-desktop` |

The OAuth client in cyber-auth must be public (`token_endpoint_auth_method: none`), use the `authorization_code` and `refresh_token` grants, and have these redirect URIs: `http://127.0.0.1:47290/callback`, `http://127.0.0.1:47291/callback`, `http://127.0.0.1:47292/callback`.

## Design

The UI follows the Cyber family design system, "Lit from within", at app density: tokens, modes
(Dark, Black, Light), primitives and patterns are documented in [`docs/DESIGN.md`](docs/DESIGN.md).
To preview the UI in a browser without the Rust core, run `pnpm dev` and open
`http://localhost:3000/?mock` (sample data, development builds only).

## Development

Requirements: Node 22 or 24 (an odd release such as 25 is refused by Next, see `engines`), pnpm, Rust (stable), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS. On Windows that means **Visual Studio 2022 Build Tools** with the *Desktop development with C++* workload (it provides `link.exe` and the Windows SDK; without it the build stops at `linker 'link.exe' not found`).

```bash
pnpm install
pnpm tauri dev          # app with hot reload
pnpm tauri build        # installers in src-tauri/target/release/bundle (needs the release signing key)
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'   # a local build without it
pnpm format             # Prettier (src/) + rustfmt (src-tauri/)
pnpm check              # formatting, types, lint, clippy
cd src-tauri && cargo test
cargo test -- --ignored --nocapture   # prints this machine's system report
```

`.gitattributes` keeps every text file LF (Windows scripts CRLF) whatever `core.autocrlf` says; a checkout made before it may need `git add --renormalize .` once. The tests that syntax-check the Linux/macOS install scripts with `sh -n` skip that part where no `sh` is on PATH (Windows without Git's `usr\bin`); their other assertions still run.

The TypeScript types of the commands' arguments and results ([`src/lib/tauri/generated/`](src/lib/tauri/generated/)) are generated from the Rust structs by [ts-rs](https://github.com/Aleph-Alpha/ts-rs): `cargo test` rewrites them, and CI fails when the committed ones differ. A command taking or returning a new struct or enum derives `ts_rs::TS` and is listed in [`testing/ts_bindings.rs`](src-tauri/src/testing/ts_bindings.rs).

### Branches

```
feature/… · fix/…  ──PR──▶  dev  ──PR──▶  main  ──tag v*──▶  release
```

- **Work** happens on a `feature/…` or `fix/…` branch cut from `dev`, merged into `dev` by pull request.
- **`dev` is staging**: everything merged there is checked together (CI, coverage, E2E, labs on each push) before it reaches production.
- **`main` is production**: it only takes pull requests from `dev`. Pushes to `main` publish the attack-box image; a `v*` tag on `main` builds and ships the release (`release.yml`, then `distribute.yml`).
- A hotfix goes into `main` by pull request, then `main` is merged back into `dev`.
