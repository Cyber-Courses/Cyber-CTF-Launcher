# Running labs

How the launcher runs a lab, wherever it runs: the forms a lab takes in each place, the files
that say where a lab is, the attack box, and the accounts and servers it reaches.

A lab describes itself once, in an `isoloom.yml`. For each place the launcher asks
[Isoloom](https://www.isoloom.com) to generate that place's files (`lab::prepare`, into the lab's
`.isoloom-<n>/<target>/`, `n` its instance number), then hands them to the Isothrum runner, which
starts, reads, changes and stops the run. The launcher keeps the policy (where, with which
account, what the player picked); the runner does the work and keeps the run's state.

## Places and forms

| Place          | Lab                                        | Isoloom target          | What runs                                                             |
| -------------- | ------------------------------------------ | ----------------------- | --------------------------------------------------------------------- |
| This machine   | containers                                 | `docker`                | Docker Compose on the engine picked at launch                         |
| This machine   | containers, "in a VM"                      | `docker-vm`             | one Vagrant VM (any local hypervisor but Hyper-V), Compose inside     |
| This machine   | VMs, or containers as "one VM per machine" | `vagrant`               | one Vagrant VM per machine                                            |
| Proxmox server | containers                                 | `docker-vm` (Proxmox)   | one VM made by Terraform, Compose inside over SSH                     |
| Proxmox server | VMs                                        | `proxmox`               | one VM per machine, made by Terraform                                 |
| ESXi server    | VMs (containers in one VM)                 | `vagrant` / `docker-vm` | Vagrant's `vmware_esxi`, the runner relaying the host from macOS      |
| Cloud account  | containers                                 | `cloud-docker`          | one instance made by Terraform, Compose inside over SSH               |
| Cloud account  | VMs, or containers as "one VM per machine" | `cloud-vm`              | one instance per machine (AWS, Azure, GCP, DigitalOcean, Linode, OCI) |
| Cloud account  | cloud services (no machines)               | `cloud-services`        | the lab's own Terraform module                                        |

The catalogue names a lab by its own runtime (containers or VMs); every operation on a started
lab finds its run through `runtime::host::locate`, which reads where it runs and the form it was
actually started in (`paths::local_runtime`).

## What says where a lab is

A running lab is the runner's run `cyberctf-<lab id>`, its state under
`<app data>/runs/<run>/` (`record.json`, `events.jsonl`, the Terraform state for a server or a
cloud). Beside the lab, in its folder:

| File                       | Says                                                                                        |
| -------------------------- | ------------------------------------------------------------------------------------------- |
| `.cyberctf-host`           | the server or cloud account it runs on (`server::mark_lab`)                                 |
| `.cyberctf-local-vm`       | a container lab here runs in a VM, and on which hypervisor                                  |
| `.cyberctf-per-machine-vm` | a container lab started as one VM per machine                                               |
| `.cyberctf-engine`         | the Docker context it was started on, when not the current one                              |
| `.cyberctf-parked`         | parked (paused or shut down): the next start resumes it                                     |
| `.cyberctf-overlay.yml`    | the machines the launcher added to the lab (the attack box), merged into its spec when read |
| `.cyberctf-instance`       | its instance number, given at install: its networks' blocks and its generated files' folder |
| `.cyberctf-known_hosts`    | the host keys of its own hosts, for their shells                                            |

A stop clears them once the run is gone (`lifecycle::forget_run`; the host keys go with the
hosts), so the next start is the lab as published. The instance number stays: it is the
installed lab's, not the run's.

## The attack box

The learner's attack box is one more machine of the lab, wherever it runs: the launcher adds
`attackbox` to the lab's spec through its overlay (`runtime/attack_machine`), regenerates the
lab for its form and brings the run up to date (`Running::update`). Taking it out retires it
from the run (`Running::retire`: Vagrant forgets a VM its Vagrantfile no longer has), removes it
from the overlay and updates again. Its status is the run's status of that machine; its shell is
the runner's console into it (`Running::console_command`).

| Form               | The box                                                                                                    |
| ------------------ | ---------------------------------------------------------------------------------------------------------- |
| Containers         | a container from the attack image picked in Settings, on the lab's network                                 |
| VMs                | a Kali VM (`kali@rolling`, 40 GB) with Kali's headless toolset, installed on its first start (~16 minutes) |
| On an Arm computer | native: Kali's arm64 image beside containers; a Debian arm64 VM made Kali by its set-up among VMs          |

A lab that declares an access machine (`access: true`) says with it where the player sits: the
box takes its place there (its name, its networks and addresses, in the overlay's `replace:`), so
the checks run from that seat run from the box; taking the box out brings the lab's own machine
back. Without one, the box is `attackbox` on the lab's first network (or the one the player
picks). Isoloom and Isothrum know nothing of the attack box: the launcher composes their general
features (`.cyberctf-overlay.yml`, update, retire, the console).

## Accounts and servers

- Profiles (non-secret): `<app data>/server.json`. Secrets: the OS keychain (service
  `org.cyberctf.desktop`, account `server:<host id>`); debug builds keep them in
  `~/.cyberctf/dev-server-secrets.json` (0600).
- A cloud account signs in either with stored keys or with its own CLI's sign-in (AWS `aws
login`, Azure `az login`, Google Cloud's application-default credentials, OCI's
  `~/.oci/config`). Before any change to a lab in an account, the launcher asks that CLI whether
  the sign-in still holds and refuses, saying how to sign in again, when it ran out
  (`runtime/isothrum/cloud.rs`): a sign-in that runs out halfway leaves a lab behind.
- A cloud lab's firewall opens its SSH and published ports to this machine's public IP only,
  which two independent services must agree on (`launch::public_ip`). AWS launches also check
  the account's budget first.
- "Test connection" checks a host without running anything: ESXi's SSH banner, Proxmox's API
  ticket, each cloud's credentials and what a launch needs (`server/cloud_checks.rs`).

### Requirements on a server

- **Proxmox**: an API token (or root with its password); `snippets` and `iso` content on the
  `local` storage; a bridge with DHCP for the lab VMs; KVM (nested virtualization when Proxmox
  is itself a VM). When the lab bridge is internal to the node, the runner reaches lab VMs
  through the node (the launcher's key let in there).
- **ESXi**: SSH on; VMware `ovftool` on this machine (for `vagrant-vmware-esxi`); promiscuous
  mode and forged transmits on the lab's port group.

`infra/test-servers/` makes a throwaway Proxmox or ESXi host; `docs/testing/integration.md` runs a lab
on it.
