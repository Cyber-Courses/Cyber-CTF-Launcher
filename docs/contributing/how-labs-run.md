# How labs run

How the launcher runs a lab, for whoever changes that code: the split between Isoloom, the
Isothrum runner and the launcher, the files that say where a lab is, how the attack box joins a
lab, how a machine's screen is reached, and what removing a lab deletes. What a player sees of all
this is in [running labs](../players/running-labs.md).

## Isoloom, the runner and the launcher

A lab describes itself once, in an `isoloom.yml`. For each place the launcher asks
[Isoloom](https://www.isoloom.com) to generate that place's files (`lab::prepare`, into the lab's
`.isoloom-<n>/<target>/`, `n` its instance number), then hands them to the Isothrum runner, which
starts, reads, changes and stops the run. The launcher keeps the policy (where, with which
account, what the player picked); the runner does the work and keeps the run's state.

| Place          | Lab                                        | Isoloom target   | What runs                                                             |
| -------------- | ------------------------------------------ | ---------------- | --------------------------------------------------------------------- |
| This machine   | containers                                 | `docker`         | Docker Compose on the engine picked at launch                         |
| This machine   | VMs, or containers as "one VM per machine" | `vagrant`        | one Vagrant VM per machine                                            |
| Proxmox server | VMs                                        | `proxmox`        | one VM per machine, made by Terraform                                 |
| ESXi server    | VMs                                        | `vagrant`        | Vagrant's `vmware_esxi`, the runner relaying the host from macOS      |
| Cloud account  | containers                                 | `cloud-docker`   | one instance made by Terraform, Compose inside over SSH               |
| Cloud account  | VMs, or containers as "one VM per machine" | `cloud-vm`       | one instance per machine (AWS, Azure, GCP, DigitalOcean, Linode, OCI) |
| Cloud account  | cloud services (no machines)               | `cloud-services` | the lab's own Terraform module and `cloud/lab.yml` (a build feature)  |

A lab runs as true containers or true VMs, never as containers inside a helper VM. A container lab
an earlier version ran inside one helper VM (Isoloom's former `docker-vm`, here, on ESXi or on
Proxmox) is still read, stopped and removed through the runner as it was started; nothing else
changes it, and its next start is in its true form (on Docker when Docker answers here, else as
one VM per machine). When that run is one this build can't read, the lab's files
are generated in the true form instead, and the log says so; when the runner no longer opens that
form at all, its helper-VM mark is cleared and the lab says which VM to remove by hand (the
Machine page lists it).

The catalogue names a lab by its own runtime (containers or VMs); every operation on a started
lab finds its run through `runtime::host::locate`, which reads where it runs and the form it was
actually started in (`paths::local_runtime`).

## What says where a lab is

A running lab is the runner's run `cyberctf-<lab id>`, its state under
`<app data>/runs/<run>/` (`record.json`, `events.jsonl`, the Terraform state for a server or a
cloud). Beside the lab, in its folder:

| File                       | Says                                                                                         |
| -------------------------- | -------------------------------------------------------------------------------------------- |
| `.cyberctf-host`           | the server or cloud account it runs on (`server::mark_lab`)                                  |
| `.cyberctf-local-vm`       | a container lab here runs in a helper VM an earlier version started, and on which hypervisor |
| `.cyberctf-per-machine-vm` | a container lab started as one VM per machine                                                |
| `.cyberctf-engine`         | the Docker context it was started on, when not the current one                               |
| `.cyberctf-parked`         | parked (paused or shut down): the next start resumes it                                      |
| `.cyberctf-overlay.yml`    | the machines the launcher added to the lab (the attack box), merged into its spec when read  |
| `.cyberctf-instance`       | its instance number, given at install: its networks' blocks and its generated files' folder  |

A stop clears them once the run is gone (`lifecycle::forget_run`), so the next start is the lab
as published. The instance number stays: it is the installed lab's, not the run's.

## The attack box in a lab

The attack box is one more machine of the lab: the launcher adds `attackbox` to the lab's spec
through its overlay (`runtime/attack_box`), regenerates the lab for its form and brings the run up
to date (`Running::update`). Taking it out retires it from the run (`Running::retire`: Vagrant
forgets a VM its Vagrantfile no longer has), removes it from the overlay and updates again. Its
status is the run's status of that machine; its shell is the runner's console into it as the
player's account (`Running::console_command` with `player`, `attack_box::PLAYER`): `exec --user`
into a container, a `sudo -i` login on a VM, whatever account the way in logs in as (`vagrant`, a
cloud image's).

A lab that declares an access machine (`access: true`) gets the box in its place (its name, its
networks and addresses, in the overlay's `replace:`), so the checks run from that seat run from
the box; taking the box out brings the lab's own machine back. Isoloom and Isothrum know nothing of
the attack box: the launcher composes their general features (`.cyberctf-overlay.yml`, update,
retire, the console).

What the box is made of comes from the [attack box repository](https://github.com/CyberCTF/attack-box),
pinned in `src-tauri/attack-box/`: the image's digest (`image.json`), the script that makes the
player's account (`player-account.sh`), and the VM set-up's parts (`vm/`), which
`attack_box::setup_script` puts together with the player's account between the toolset and the
desktop. The account takes the shared folder's owner's uid where the box doesn't have that user (a
Linux computer's, through Docker or virtiofs), and joins the folder's group (a VM's shared folder
mounted for `vagrant`). Root stays the image's own user.

### Its password

The launcher makes a new password each time it adds a box (`attack_box/sign_in.rs`): 20 random
characters of lowercase letters and digits without look-alikes, in four groups
(`k7mpq-zrt4w-hn8xc-ls2vd`, 100 bits). It keeps it with its other secrets (the OS keychain,
`attack-box:<lab>`; a 0600 file in a dev build), registers it with the log redaction, and forgets
it when the box is taken out. The box gets only its SHA-512 crypt hash (RustCrypto's `sha-crypt`,
its default 5000 rounds, a 16-character salt from the system's random generator), as a root-only
file of its spec (`files: [{ path: /etc/cyberctf/player-password, content, mode: "0400" }]`, a
Compose config for a container, Isoloom's files step before a VM's set-up), which
`player-account.sh` hands to `chpasswd -e` on its standard input: the password is in none of the
lab's or the run's files, and neither it nor its hash is on a command line. A container box sets
it at each start, as it makes the account. The lab page and the Screen window read it through
`attack_box_sign_in` and `screen_sign_in`.

## A machine's screen

Isoloom writes, for each VM, which graphical console it has under each provider and why there is
none where there isn't (`machines[].consoles`, its
[capability matrix](https://github.com/isoloom/isoloom/blob/main/docs/matrix.md#graphical-consoles));
the lab page shows that reason instead of a Screen button. Isothrum reaches the console
(`Running::graphical_console`) on this computer's loopback, opening an SSH tunnel where the
console sits behind a machine of the run (a cloud Windows VM's Remote Desktop).

- **VNC**: the window draws it with noVNC (vendored, `src/vendor/novnc/`). The webview never
  reaches the console itself: the window's own bridge (`screen/bridge.rs`), a WebSocket server on
  `127.0.0.1` at a free port, relays it. The bridge is bound when the window opens, and the
  window's content security policy lets it connect to that port alone
  (`connect-src ws://127.0.0.1:<port>`, added to that window's pages only). It takes one viewer,
  with a one-time token, from the app's own page (its `Origin`, exactly) addressed to its own
  loopback address (its `Host`): a website, or another program without the token, gets nothing.
  Proxmox's console is its API's VNC websocket, which the bridge opens with the server's
  credentials and the session's ticket.
- **RDP**: the window opens the system's client on a `.rdp` file naming the loopback address and
  the account, never a password. The account's password stays in the launcher until the player
  copies it; only FreeRDP is given it, on its standard input. The `.rdp` file is owner-only, and
  goes with the window.

Closing the window ends its session: the bridge stops, and a tunnel to the console closes.

## Removing a lab

`labs::remove` deletes, once nothing of the lab is up (running, starting, parked, or machines left
by a start that was cut off) and no operation runs on it; the core refuses otherwise, called from
the app or not:

- the copy, `<app data>/labs/<lab id>/`, with the project generated in it (Compose files,
  Vagrantfiles, Terraform modules), and an update's or an import's leftover `.<lab id>.staging` and
  `.<lab id>.old`;
- the runner's run folder, `<app data>/runs/cyberctf-<lab id>/` (its record, its event log, what
  it last saw, Vagrant's state of it);
- the Terraform deployments an older launcher left in `<app data>/deployments/<lab id>/` (none of
  them still holding resources: that counts as the lab still being up);
- its deploy worker's job, logs and verdicts in `<app data>/deploys/`;
- a catalogue lab's source kept for an offline start, `<app data>/offline/<lab id>.json`;
- the password kept for its attack box (the OS keychain's `attack-box:<lab id>`), once the rest is
  gone. A keychain that refuses (locked) leaves it, listed with why, and the lab is removed all the
  same; no keychain at all (Linux without a Secret Service) has nothing to delete.

The copy is first moved aside whole: a file in it held open (Windows) refuses the removal with
nothing deleted. What can't be deleted after that is listed with why; the lab is gone from the app
either way. The lab's empty lock file in `deploys/` stays (every operation on the lab shares it, so
deleting it could let two run at once). The folder an imported lab came from is never touched: the
copy keeps no link out of itself, and a removal deletes a link, never what it points at.

## How the player reaches a lab

A lab is reached from this computer three ways: its link (the page of its first published web
port), a shell into a machine ("Open shell", the attack box's included), and its checks. Where the
lab runs decides how, once, as one strategy every way in asks (`runtime/isothrum/reach.rs`,
`Reach`) rather than an `if` of its own:

- **Direct**: the lab's addresses are routed here. A local lab, a server whose lab bridge is on
  its network, a cloud's public addresses, an ESXi host's VMs.
- **Through the node**: only a node of the server reaches them: a Proxmox server whose lab bridge
  is internal to its node. Every way in goes through the node first.

**The choice.** Made where the lab's place is: the server's profile (its kind, and whether its lab
bridge is internal, which `server::proxmox_jump_for_host` reads from the node's interfaces) gives
the node's login (`root@<node>`), when an operation applies the module (`Reach::of_place`). The run
records it among its values as `ssh_jump` (Isothrum's: the host every console and command of the
run goes through first), so what only reads the run (its status, a shell, its checks) finds it
there (`Reach::of_record`) without asking the server again.

**Through the node.** Every hop signs in with the runner's key, which the node lets in for the jump
(`server::proxmox_jump_for_host` authorizes it), and the node's host key is the one the player
confirmed in Servers (`ssh::node_known_hosts`):

- Terraform's own SSH to the router, while an operation applies the module: a SOCKS proxy through
  the node (`isothrum/tunnel.rs`, the module's `ssh_proxy_host` and `ssh_proxy_port`), for that
  operation only.
- The shell: an SSH whose `ProxyCommand` is the router's, itself reached through the node's
  (`isothrum/opened.rs`), as the module's `ssh_user`, then `sudo -iu` the account asked for. The
  machine's address is the runner's, which takes it from the module's `machines` output where the
  state has none (a Proxmox VM without a guest agent).

## Servers and cloud accounts

- Profiles (non-secret): `<app data>/server.json`. Secrets: the OS keychain (service
  `org.cyberctf.desktop`, account `server:<host id>`); debug builds keep them in
  `~/.cyberctf/dev-server-secrets.json` (0600).
- Before any change to a lab in a cloud account, the launcher asks that cloud's CLI whether its
  sign-in still holds, and refuses, saying how to sign in again, when it ran out
  (`runtime/isothrum/cloud_runs.rs`): a sign-in that runs out halfway leaves a lab behind.
- A cloud lab's firewall opens its SSH and published ports to this machine's public IP only,
  which two independent services must agree on (`launch::public_ip`). AWS launches also check the
  account's budget first.
- "Test" checks a host without running anything: ESXi's SSH banner, Proxmox's API ticket, each
  cloud's credentials and what a launch needs (`server/cloud_checks.rs`).
- The "VM test" on a Proxmox or ESXi host runs a throwaway one-machine lab
  (`cyberctf-server-selftest-<host id>`) through the path a lab takes there, and always removes it
  (`runtime/server_selftest/`). On Proxmox, the lab is generated for the `proxmox` target; on ESXi,
  for `vagrant`, run with `vmware_esxi`, after checking `vagrant-vmware-esxi` and `ovftool` here.
  A test whose teardown failed or was cut short keeps its folder, and the next test removes that
  lab first.
- The OVF Tool for ESXi is found in this order: Fusion's copy
  (`/Applications/VMware Fusion.app/Contents/Library/VMware OVF Tool`) or Workstation's
  (`VMware\VMware Workstation\OVFTool` under Program Files), then the launcher's own copy
  (`tools/ovftool/` in its app data), then the first `ovftool` on `PATH`. The launcher's `PATH`
  lists those folders in that order from startup, so a copy placed while it runs is used by the
  next lab without a restart; on macOS the runner's ovftool wrapper for ESXi hands over to that
  same first `ovftool`. On macOS, a picked folder is checked on the copy (the program and every
  library validly signed by VMware's Developer ID, team `EG7KH642X6`: `codesign --verify --strict`,
  then the same against a requirement naming that team) before the copy's `com.apple.quarantine`
  attribute is removed.

[Integration tests](testing/integration.md) run a lab on a Proxmox or ESXi host given by its
settings.
