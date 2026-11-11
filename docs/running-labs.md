# Running labs

How the launcher runs a lab, wherever it runs: the forms a lab takes in each place, the files
that say where a lab is, the attack box, the labs imported from a folder, and the accounts and
servers it reaches.

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

A stop clears them once the run is gone (`lifecycle::forget_run`), so the next start is the lab
as published. The instance number stays: it is the installed lab's, not the run's.

## The attack box

The learner's attack box is one more machine of the lab, wherever it runs: the launcher adds
`attackbox` to the lab's spec through its overlay (`runtime/attack_machine`), regenerates the
lab for its form and brings the run up to date (`Running::update`). Taking it out retires it
from the run (`Running::retire`: Vagrant forgets a VM its Vagrantfile no longer has), removes it
from the overlay and updates again. Its status is the run's status of that machine; its shell is
the runner's console into it as the player's account (`Running::console_command` with `player`,
`attack_machine::PLAYER`): `exec --user` into a container, a `sudo -i` login on a VM, whatever
account the way in logs in as (`vagrant`, a cloud image's).

The box has that account on every form, with a home and sudo without a password
(`images/attack-box/player-account.sh`): the attack image is built with it, a container box makes
it where it starts (its image may predate it), and a VM box at the end of its set-up. Where the
shared folder belongs to a user the box doesn't have (a Linux computer's, through Docker or
virtiofs), the account takes that user's uid, and it joins the folder's group (a VM's shared
folder mounted for `vagrant`), so it writes there as that user or group may. Root stays the
image's own user.

The account also has a password, for wherever the box asks for one: a desktop box's screen locker
or LightDM's login after a logout, a VM's console login on its Screen, `su`, SSH where the box's
`sshd` takes passwords. The launcher makes a new one each time it adds a box
(`attack_machine/sign_in.rs`): 20 random characters of lowercase letters and digits without
look-alikes, in four groups (`k7mpq-zrt4w-hn8xc-ls2vd`, 100 bits). It keeps it with its other
secrets (the OS keychain, `attack-box:<lab>`; a 0600 file in a dev build), registers it with the
log redaction, and forgets it when the box is taken out. The box gets only its SHA-512 crypt hash,
as a root-only file of its spec (`files: [{ path: /etc/cyberctf/player-password, content, mode:
"0400" }]`, a Compose config for a container, Isoloom's files step before a VM's set-up), which
`player-account.sh` hands to `chpasswd -e` on its standard input: the password is in none of the
lab's or the run's files, and neither it nor its hash is on a command line. A container box sets
it at each start, as it makes the account. The lab page shows it in the attack box's
**Credentials** row (masked until **Show**, copied with a click), and the box's Screen window in
its toolbar's **Sign-in** popover (`attack_box_sign_in`, `screen_sign_in`). Sudo still asks for
none. A box added before this has no password, and no Credentials row.

| Form               | The box                                                                                                                                                   |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Containers         | a container from the attack image picked in Settings, on the lab's network                                                                                |
| VMs                | a Kali VM (`kali@rolling`, 40 GB) with Kali's headless toolset, installed on its first start (~16 minutes); with Kali's XFCE desktop when Settings say so |
| On an Arm computer | native: Kali's arm64 image beside containers; a Debian arm64 VM made Kali by its set-up among VMs                                                         |

A lab that declares an access machine (`access: true`) says with it where the player sits: the
box takes its place there (its name, its networks and addresses, in the overlay's `replace:`), so
the checks run from that seat run from the box; taking the box out brings the lab's own machine
back. Without one, the box is `attackbox` on the lab's first network (or the one the player
picks). Isoloom and Isothrum know nothing of the attack box: the launcher composes their general
features (`.cyberctf-overlay.yml`, update, retire, the console).

A VM box runs headless unless Settings choose its desktop (Settings, Attack box, "Attack VM
desktop"): its set-up then installs Kali's XFCE desktop and LightDM, which signs `player` in
(about 1 GB more to download, 3 GB more on its disk), and the VM gets 4 GB of memory and 2 CPUs.
Its Screen shows that desktop. The `player` account, its sudo rule and its password stay as they
are. The desktop never locks the player out: neither of Kali's screen lockers (`light-locker`,
`xfce4-screensaver`) starts for `player`, the X server never blanks the screen, and at each
sign-in the session turns off the screensaver's lock, the power manager's lock on suspend and the
session's lock on a switch of user (`/usr/local/bin/cyberctf-no-screen-lock`). Should a prompt
still show (a logout, a locker started by hand), the password takes it. A container box is
always headless: no desktop is offered for it.

## A machine's screen

A lab VM whose run offers a graphical console (its screen, keyboard and mouse) has a Screen
button on the lab page (Screens, and the attack box's card): it opens the machine's screen in its
own window, beside the attack box's shell window. Isoloom writes, for each VM, which console it
has under each provider and why there is none where there isn't
(`machines[].consoles`, its [capability matrix](https://github.com/isoloom/isoloom/blob/main/docs/matrix.md#graphical-consoles));
the lab page shows that reason instead of a button. Isothrum reaches it
(`Running::graphical_console`) on this computer's loopback, opening an SSH tunnel where the
console sits behind a machine of the run (a cloud Windows VM's Remote Desktop).

- **VNC** (libvirt, QEMU, VMware Workstation and Fusion, Proxmox):
  the window draws it with noVNC (vendored, `src/vendor/novnc/`). The webview never reaches the
  console itself: the window's own bridge (`screen/bridge.rs`), a WebSocket server on
  `127.0.0.1` at a free port, relays it. The bridge is bound when the window opens, and the
  window's content security policy lets it connect to that port alone
  (`connect-src ws://127.0.0.1:<port>`, added to that window's pages only). It takes one viewer,
  with a one-time token, from the app's own page (its `Origin`, exactly) addressed to its own
  loopback address (its `Host`): a website, or another program without the token, gets nothing.
  Proxmox's console is its API's VNC websocket, which the bridge opens with the server's
  credentials and the session's ticket.
- **RDP** (VirtualBox's remote display, a cloud Windows VM's Remote Desktop): the window opens
  the system's client on a `.rdp` file naming the loopback address and the account, never a
  password: Windows' Remote Desktop Connection, on a Mac Windows App (or Microsoft Remote
  Desktop), on Linux FreeRDP or Remmina; the window says what to install when there is none.
  The account's password stays in the launcher until the learner copies it ("Copy password");
  only FreeRDP is given it, on its standard input. The `.rdp` file is owner-only, and goes with
  the window.

Closing the window ends its session: the bridge stops, and a tunnel to the console closes.

## Labs imported from a folder

**Labs > Import a lab** runs a lab that isn't published: pick a folder with an `isoloom.yml`, and
the launcher copies it into `<app data>/labs/local-<name>/` (`name` is the spec's) and starts it as
containers on this machine, without Cyber CTF (no sign-in, no flags, no progress saved). It is
listed under **Imported from a folder** on the Labs screen, under **Running** in the sidebar and on
the Overview, and each opens its lab page.

No catalogue entry describes it, so its page shows what the lab's own `.ctf/` folder defines, read
from the installed copy (the one the run uses), each file on its own:

| File                   | On the page                                                                                                                                           |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `.ctf/metadata.json`   | the title, description, difficulty (1 to 3), category and capabilities (as tags); without it, the title is the spec's `name` and the rest is left out |
| `.ctf/objectives.json` | the objectives (`{ "objectives": [...] }`): each one's title, prompt and guided steps, to read only; without it, no objectives section                |

The objectives are read only: progress and answers are checked and kept by Cyber CTF for
published labs, so an imported lab's page says so instead of offering an answer field. The
answers (`check`, `dev`) and the hint texts never leave the core. A file that is there but can't
be read (not JSON, a missing `title`, a difficulty out of range, a key used twice) is named on the
page with why ("metadata.json can't be read: …"); the other file is still read.

Without a `.ctf/` folder the page is the lab itself: its name, state, deployment steps and log,
network diagram, where it runs, the attack box, and Stop, Open in browser and the shell. A stopped
imported lab starts again by importing its folder again, which picks up any change made to it.

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
- "Test" checks a host without running anything: ESXi's SSH banner, Proxmox's API
  ticket, each cloud's credentials and what a launch needs (`server/cloud_checks.rs`).
- The "VM test" on a Proxmox or ESXi host runs a throwaway one-machine lab
  (`cyberctf-server-selftest-<host id>`) through the path a lab takes there, and always removes
  it (`runtime/server_selftest/`). On Proxmox, the lab is generated for the `proxmox` target. On
  ESXi, it is generated for `vagrant` and run with `vmware_esxi`. The ESXi test first checks
  `vagrant-vmware-esxi` and `ovftool` on this machine. Its upload step names the box and its
  architecture. A test whose teardown failed or was cut short keeps its folder, and the next test
  removes that lab first.

### Requirements on a server

- **Proxmox**: an API token (or root with its password); `snippets` and `iso` content on the
  `local` storage; a bridge with DHCP for the lab VMs; KVM (nested virtualization when Proxmox
  is itself a VM). When the lab bridge is internal to the node, the runner reaches lab VMs
  through the node (the launcher's key let in there).
- **ESXi**: SSH on; VMware `ovftool` on this machine (for `vagrant-vmware-esxi`); promiscuous
  mode and forged transmits on the lab's port group.

### The OVF Tool for ESXi

`vagrant-vmware-esxi` uploads a lab's VMs to ESXi with VMware's OVF Tool (`ovftool`). The ESXi
setup's tools step offers three ways to get it:

- **Get VMware Fusion** (macOS) or **Get VMware Workstation** (Windows and Linux on x64): both are
  free from Broadcom and ship the OVF Tool inside them, where the launcher looks for it.
- **Get the standalone OVF Tool**: Broadcom's download, an archive that unpacks to a folder
  (`ovftool`, `lib/`, `env/`, `certs/`, …) that nothing looks in.
- **Use a downloaded OVF Tool…**: pick that unpacked folder (or the one folder inside it that
  holds `ovftool`). The launcher copies it into `tools/ovftool/` in its app data folder, replacing
  an earlier copy, and leaves the download where it is. It refuses a folder without the
  `ovftool` program (`ovftool.exe` on Windows) and its libraries (`lib/` beside it), or one far
  bigger than the tool. On macOS it first checks, on the copy, that the program and every library
  are validly signed by VMware's Developer ID (team `EG7KH642X6`: `codesign --verify --strict`,
  then the same against a requirement naming that team), refuses anything else, and only then
  removes the copy's `com.apple.quarantine` attribute: the standalone tool isn't notarized, so
  Gatekeeper would refuse to start it otherwise. The download keeps its quarantine. Tool installs
  turned off by policy (`DisableToolInstalls`) turn this off too.

The standalone macOS tool is built for Intel Macs only: on Apple Silicon it runs under Rosetta,
and the tools step says so when Rosetta isn't installed (`softwareupdate --install-rosetta`).

The machine check looks for the tool in this order and shows the version it found and where:
Fusion's copy (`/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool`) or
Workstation's (`VMware\VMware Workstation\OVFTool` under Program Files), then the launcher's own
copy, then the first `ovftool` on `PATH`. The launcher's `PATH` lists those folders in the same
order, from startup, so a copy placed while it runs is used by the next lab without a restart; on
macOS the runner's ovftool wrapper for ESXi hands over to that same first `ovftool` on `PATH`. An
`ovftool` in a folder already on your shell's `PATH` (when the launcher is started from a
terminal) comes before them all when a lab runs.

`infra/test-servers/` makes a throwaway Proxmox or ESXi host; `docs/testing/integration.md` runs a lab
on it.
