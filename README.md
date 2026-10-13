# Cyber CTF Desktop

Desktop launcher for Cyber CTF labs, built with [Tauri 2](https://tauri.app). It runs **Docker labs** (Docker Compose) and **VM labs** (Vagrant, on any supported hypervisor) on the player's machine, and labs on the player's own servers (ESXi, Proxmox) and cloud accounts. This app replaced the earlier Electron launcher in this repository.

## Architecture

- **UI:** Next.js as a **static export** (`output: "export"`) served by Tauri. There is no Next server, so no API routes, SSR or middleware. The UI only calls typed Rust commands ([`src/lib/tauri/`](src/lib/tauri/), one module per area).
  - [`src/features/`](src/features/): one folder per page with its parts and hooks: `labs` (catalogue, lab page, deployment steps, network diagram), `machine` (Machine page and the setup steps), `servers`, `cloud`, `settings`, `home`, `hosted`, `onboarding`, `account`.
  - [`src/components/ui/`](src/components/ui/): shared building blocks (Button, Panel, RadioRow, Segmented, ChoiceCard, StatusPill, LogConsole…); [`src/components/`](src/components/) holds the app shell.
  - [`src/lib/`](src/lib/): data access ([`tauri/`](src/lib/tauri/)), per-machine settings, formatting ([`format.ts`](src/lib/format.ts)) and small helpers.
- **Core (Rust, [`src-tauri/`](src-tauri/)):** everything privileged. Processes are spawned without a shell, lab ids are validated, and the UI never passes paths or command lines.
  - [`account/`](src-tauri/src/account/): Cyber Auth sign-in (`auth.rs`: authorization code + PKCE through the system browser, loopback redirect on `127.0.0.1:47290-47292/callback`; tokens in the OS keychain, refreshed automatically, never in the webview), CyberBackend GraphQL with the token attached on the Rust side (`api.rs`), and the agent that links the launcher to the website (`agent.rs`, `colocation.rs`).
  - [`labs.rs`](src-tauri/src/labs.rs): `lab_launch` calls `startLab`, then downloads `CyberCTF/*` at the pinned commit (full SHA only; archive paths checked), and starts the lab with `CTF_API_URL` and `CTF_LAUNCH_TOKEN`. The lab's `evidence` service exchanges the token for this player's evidence.
  - [`machine/`](src-tauri/src/machine/): what this machine can run (`system/`: Docker engines, Vagrant, hypervisors, package manager), the setup self-tests (`selftest/`) and the workloads/storage the Machine page shows (`workloads/`, `storage.rs`).
  - [`platform/`](src-tauri/src/platform/): the PATH a GUI launch is missing (`env_path.rs`) and one-click dependency installs (`install.rs`).
  - [`runtime/`](src-tauri/src/runtime/): lab lifecycle, with logs streamed to the UI over a Tauri channel
    - `lab.rs`: the lab's Isoloom side; the target's files (Compose, Vagrant, Terraform) are generated from its `isoloom.yml` with `isoloom-core` right before they run
    - `launch.rs`, `lifecycle.rs`, `status.rs`: start, park/resume, stop and the auto-stop reaper for cloud labs, and what is running where
    - `docker/`: `docker compose -p cyberctf-<lab>`; stop removes volumes, so every start is a clean lab; status reports machines, interfaces, networks and declared services
    - `vm.rs` + `vm/`: `vagrant up --provider <p>`; stop destroys the VMs
    - `server/`, `proxmox.rs`, `terraform/`: labs on the player's own servers (ESXi, Proxmox) and cloud accounts (AWS, Azure, GCP, DigitalOcean, Linode, OCI); `server_selftest/` checks a server or account before the first lab
    - `providers.rs`: Vagrant providers. Local: VirtualBox, VMware Desktop, Hyper-V, Parallels, libvirt, QEMU, UTM. Remote: VMware ESXi, Proxmox.
    - `exegol.rs`, `attack_box.rs`, `attack_vm.rs` + `attack_vm/`: the attack box (container) or attack VM beside the lab
  - [`deploy_worker/`](src-tauri/src/deploy_worker/): launches, resumes, parks and provisioning runs in a detached worker process (`cyberctf-desktop deploy --job <file>`), so quitting the app never cuts a deploy short. A per-lab file lock keeps the app and the workers from running two operations on one lab at once.
  - [`lan_relay/`](src-tauri/src/lan_relay/): loopback relays that let Vagrant and ovftool reach a LAN ESXi host on macOS.
  - `terminal.rs`: the in-app attack box shell (a PTY streamed to xterm.js); `shared_folder.rs`: the folder shared with the attack box.
  - `cloud.rs`: cloud account sign-in through each provider's CLI; [`exec/`](src-tauri/src/exec/), `http.rs` (HTTP clients with timeouts and a download size cap), `error.rs`, `config.rs`: crate-wide basics.

The runtime layer knows nothing about CTFs: a lab is a directory with an `isoloom.yml` (its machines, networks, services and checks). The launcher generates the files for the target it runs on under the lab's `.isoloom/` folder.

Labs live in `<app data>/labs/<lab id>/`.

### Platform notes

A VM lab runs natively only where its boxes exist for the host architecture. On Apple Silicon, ARM boxes run natively on UTM/QEMU (Apple Hypervisor). x86-only boxes such as pfSense run there only under QEMU emulation, which is several times slower; heavy x86 labs should use a remote x86 host (ESXi, Proxmox).

Hyper-V only takes management commands from an administrator (or a member of "Hyper-V Administrators"), and Vagrant's Hyper-V provider refuses to run otherwise. On Windows, Vagrant commands on Hyper-V machines run through a generated, hidden Windows PowerShell script ([`exec/elevate.rs`](src-tauri/src/exec/elevate.rs)); when the launcher itself may not manage Hyper-V, Windows first asks for administrator rights (a UAC prompt, raised for the app's window). Status reads are never elevated.

- Prompts: one per batch of Vagrant commands. A lab's own start (boot, then power off the setup machine), provisioning run, pause or shutdown, resume and stop are one batch each. A VM lab with an attack VM asks once more for the attack VM in each of these, and again for each retry of an attack VM start that hit Vagrant's machine lock. The VM self-test asks twice (boot and run a command, then delete the test VM), three times when it first deletes a test VM left from an earlier run. Started as administrator (or as a Hyper-V administrator), the launcher asks nothing.
- Output streams as usual: the script writes it to a log the launcher follows. The environment the commands need (tokens included) goes through a file in the batch's own temp folder, read and deleted by the script, never on a command line.
- Vagrant's questions: on Windows, Vagrant waits forever for an answer even with no one to give it. The launcher's own Vagrantfiles (self-test, attack VM) attach to a virtual switch the script picks ("Default Switch", else an external one, else the first), and the attack VM gets no shared folder on Hyper-V (Vagrant would ask for a Windows password to share it over SMB). If Vagrant asks anyway (a lab's machines with several switches and none named), the batch is stopped with a message saying so.
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

Production endpoints are built in ([`config.rs`](src-tauri/src/config.rs)). Each can be overridden at build or run time, e.g. to test against a local cyber-auth:

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

Requirements: Node 22+, pnpm, Rust (stable), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
pnpm install
pnpm tauri dev          # app with hot reload
pnpm tauri build        # installers in src-tauri/target/release/bundle
pnpm format             # Prettier (src/) + rustfmt (src-tauri/)
pnpm check              # formatting, types, lint, clippy
cd src-tauri && cargo test
cargo test -- --ignored --nocapture   # prints this machine's system report
```

The TypeScript types of the commands' arguments and results ([`src/lib/tauri/generated/`](src/lib/tauri/generated/)) are generated from the Rust structs by [ts-rs](https://github.com/Aleph-Alpha/ts-rs): `cargo test` rewrites them, and CI fails when the committed ones differ. A command taking or returning a new struct or enum derives `ts_rs::TS` and is listed in [`ts_bindings.rs`](src-tauri/src/ts_bindings.rs).
