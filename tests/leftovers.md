# What the stack leaves on a machine

The contract of the leftover audit: everything the launcher, Isothrum and the projects Isoloom
generates can make on a machine, kind by kind, with what of each is kept by design. After a run
that started and stopped labs, nothing else may be left. `tests/audit/` checks it: a snapshot
before, one after, and their diff, one verdict a kind.

```sh
tests/audit/run.sh snapshot --out before.json [--root <the run's folder>]   # Linux, macOS
pwsh tests/audit/snapshot.ps1 -Out before.json [-Root <folder>]            # Windows
ssh root@<server> sh -s < tests/audit/snapshot-server.sh > before.json     # Proxmox VE, ESXi
# ... the run ...
tests/audit/run.sh diff before.json after.json --mode clone --results results.jsonl
tests/audit/run.sh --list                                                   # the kinds, as JSON lines
```

A snapshot is read-only and takes a few seconds: each tool is asked once, within a time limit,
and a tool that is missing, refuses (`--sudo`: what needs root, through `sudo -n`) or doesn't
answer makes that kind a `skip` saying which, never an error. It records each tool's own output;
`tests/audit/inventory.ts` reads them (`inventory.test.ts` holds each parser to a recorded output
in `tests/audit/fixtures/`).

## Whose a new thing is

- **A throwaway clone** (`--mode clone`, the default): everything new is the run's.
- **A machine someone uses** (`--mode shared`, the macOS lane on the owner's Mac): the owner's own
  labs, VMs, containers and files exist and change while the run runs, and are never reported or
  touched. A new thing is the run's only when the run proves it made it:
  - it lies under one of the run's folders (`--root`: `CYBERCTF_WDIO_ROOT`, a runner's work
    folder; a container's Compose files, a Vagrant machine's folder, a process's arguments);
  - or the run used its name (`--name`, or the run's logs name it, `--named-in`) and it was made
    while the run ran (`--window <start>:<end>`, epoch seconds, a minute's grace after). A window
    given its own logs (`--window <s>:<e>=<log>,…`, a pass's) needs them to name it: a Compose
    project's name is a lab's, and another process can make one of the same name meanwhile. With
    no window, or no date for the thing, the name decides;
  - a listening socket is the run's when its process is.

  With none of these, nothing is the run's: never a blanket diff of the machine.

What the run may leave is `tests/audit/allowlist.tsv`, each entry with its reason (a caller adds
its own with `--allow`). Everything new that is neither allowed nor the run's is listed as "not the
run's" in the report and left alone.

## Results

`--results <file>` appends one record a kind, as the other entry points ([README.md](README.md),
"Results"), suite `audit`, the kind its case:

```json
{
  "suite": "audit",
  "case": "docker-network",
  "provider": "",
  "form": "",
  "result": "fail",
  "reason": "left 1: cyberctf-hello_lab",
  "seconds": 0,
  "log": "…/audit-report.txt"
}
```

`fail` names what was left (twenty names, then how many more); `skip` says which tool couldn't
answer and when (before or after the run); `pass` says what it allowed and what wasn't the run's.
The exit status is 1 when a kind failed, 2 when the audit couldn't run.

## The inventory

Names: `<id>` a lab's id; `<run>` Isothrum's run, `cyberctf-<id>` for the launcher; `<env>` the
lab's Isoloom environment (its `isoloom.yml` name, `-<n>` for an instance); `<slot>` a server
slot (1 to 99); `<data>` the launcher's data folder (`org.cyberctf.desktop` under
`~/.local/share`, `~/Library/Application Support` or `%APPDATA%`), its cache folder alike.

### Docker

| Kind               | What the stack makes                                                                                                                                                                                                                                                                                                                                                 | Kept by design                                                                                |
| ------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `docker-container` | a lab's Compose project `cyberctf-<id>` (label `com.docker.compose.project`), containers `cyberctf-<id>-<service>-<n>`, Isoloom's helpers (`isoloom-router`, `-probe-<arch>`, `-check`, `-init-…`, `-access`), the attack box (service `attackbox`); the guided setup's self-test project `cyberctf-selftest`; labels `isoloom.managed`, `isoloom.environment=<env>` | none                                                                                          |
| `docker-network`   | `cyberctf-<id>_<network>`, `_isoloom-mgmt`, `_isoloom-trunk-<lan>-<machine>` (Linux bridges `br-<id>`, with Docker's iptables rules)                                                                                                                                                                                                                                 | none                                                                                          |
| `docker-volume`    | `cyberctf-<id>_<volume>`, `_<disk>`, `_isoloom-probe-<arch>`; anonymous volumes                                                                                                                                                                                                                                                                                      | a volume labelled `isoloom.persistent=true` outlives a stop                                   |
| `docker-image`     | pulled: a lab's images, Isoloom's helpers (`curlimages/curl`, `busybox`, `alpine`, `nicolaka/netshoot`, `hashicorp/terraform`), the attack box `cyberctf/attack-box`; built: `localhost/isoloom/<env>-<machine>`, `<run>-<service>`                                                                                                                                  | all: the launcher's cache, recorded in `storage.json`, removed by the Machine page's Clean up |

A container lab in a VM (`docker-vm`) runs Compose inside the VM (project `<env>`): on the machine
it leaves only the VM and its port forwards.

### Vagrant and the hypervisors

| Kind                | What the stack makes                                                                                                                                                                                                                                           | Kept by design                                                                   |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `vagrant-machine`   | an entry of Vagrant's machine index (`global-status`) a machine, its Vagrantfile in `<data>/labs/<id>/.isoloom[-n]/{vagrant,docker-vm}/`, its state in `<data>/runs/cyberctf-<id>/vagrant` (`VAGRANT_DOTFILE_PATH`); the self-test's in `<cache>/selftest/vm/` | none                                                                             |
| `vagrant-box`       | the boxes a lab or the self-test uses (`bento/*`, `generic/*`, `cloud-image/debian-12`, `utm/bookworm`)                                                                                                                                                        | all: the cache                                                                   |
| `vagrant-dotfolder` | a `.vagrant` folder under `<data>`, `<cache>` or the run's folders: the self-test's, and Isoloom's QEMU hub's (`.isoloom/vagrant/.vagrant/isoloom-qemu-hubs/<net>.pid`, `.log`)                                                                                | none                                                                             |
| `libvirt-domain`    | `<env>_<machine>`, `<env>_default` (a container lab in a VM), the self-test's `cyberctf-…` (`qemu:///system`, or `LIBVIRT_DEFAULT_URI`)                                                                                                                        | none                                                                             |
| `libvirt-network`   | `isoloom-<env>-<net>` a lab network; vagrant-libvirt's `vagrant-libvirt` management network                                                                                                                                                                    | none                                                                             |
| `libvirt-pool`      | none (the `default` pool is used)                                                                                                                                                                                                                              | none                                                                             |
| `libvirt-volume`    | a domain's disk and data disks                                                                                                                                                                                                                                 | a box's image (`*_vagrant_box_image_*`): the cache                               |
| `vbox-vm`           | `<env> - <machine>`, `<env> - isoloom-docker-host`, the self-test's `CyberCTF test VM`                                                                                                                                                                         | the linked-clone master Vagrant imports from a box (`<box>_<ms>_<n>`): the cache |
| `vbox-hostonly`     | the self-test's private network (192.168.56.0/24) makes `vboxnet<n>` (VirtualBox 7 on a Mac: `vagrantnet-*`); a lab's networks are internal (`intnet`) and make none                                                                                           | the host-only adapter: Vagrant never removes it, the next VM reuses it           |
| `vbox-dhcp`         | the host-only adapter's `HostInterfaceNetworking-vboxnet<n>`                                                                                                                                                                                                   | the same                                                                         |
| `vbox-natnet`       | none                                                                                                                                                                                                                                                           | none                                                                             |
| `utm-vm`            | `<env> - isoloom-docker-host` (UTM runs container labs in a VM only)                                                                                                                                                                                           | none                                                                             |
| `parallels-vm`      | `<env> - <machine>` (linked clones)                                                                                                                                                                                                                            | the linked-clone master                                                          |
| `hyperv-vm`         | `<env>-<machine>`, the self-test's `CyberCTF test VM`                                                                                                                                                                                                          | none                                                                             |
| `hyperv-switch`     | a private switch `isoloom-<env>-<net>` a lab network, removed with the environment's last VM                                                                                                                                                                   | the Default Switch (or `ISOLOOM_HYPERV_SWITCH`) is used, not made                |
| `hyperv-nat`        | none                                                                                                                                                                                                                                                           | none                                                                             |
| `hyperv-vhd`        | a VM's differencing disk (in its `.vagrant` folder)                                                                                                                                                                                                            | a box's parent disk                                                              |

VMware Desktop's and Parallels' host-only networks for a lab's private networks are their plugins'
defaults, with no clean-up in either repository: a run on them is the first to say.

### Processes, ports and files

| Kind              | What the stack makes                                                                                                                                                                                                                                                                                                                                                                                                                 | Kept by design                                                                                                                         |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| `process`         | the app; its deploy workers (`<app> deploy --job <data>/deploys/<id>.json`, their own process group, outliving the app until the operation ends) and their `vagrant`, `docker compose`, `terraform`; the console's SSH tunnel (`ssh -N -L 127.0.0.1:…`); the VM processes (`qemu-system-*`, `VBoxHeadless`, `vmwp`); Isoloom's QEMU hub (`qemu-system-* -machine none -daemonize`); an RDP client given `<data>/screen/<window>.rdp` | none once the labs are stopped                                                                                                         |
| `listening-port`  | a lab's published ports (`127.0.0.1:<port>`, `docker-proxy` on Linux), the Screen's VNC bridge (`127.0.0.1:<random>`), the sign-in callback (`127.0.0.1:47290`–`47292`), the QEMU hub's (20000–39999), Vagrant's forwarded ports, the wdio build's WebDriver (`TAURI_WEBDRIVER_PORT`)                                                                                                                                                | none                                                                                                                                   |
| `temp-folder`     | the desktop runs' folders (`cyberctf-e2e.*`), the tests' (`cyberctf-lab-*`, `cyberctf-box-*-home`, `isoloom-cli-*`), Windows' `cyberctf-vagrant-<hex>` and `cyberctf-shell-*.cmd`, Isoloom's `isoloom-project*.tgz`, the single-instance socket `org_cyberctf_desktop_si.sock` (macOS)                                                                                                                                               | none                                                                                                                                   |
| `run-file`        | under `<data>`, `<cache>`, `~/.isothrum` and the run's folders: the deploy workers' `deploys/<id>.pid` and `.lock`, Isothrum's `runs/<run>/lock`, the QEMU hub's pid files                                                                                                                                                                                                                                                           | the app's own locks (`storage.lock`, `installed-tools.lock`, `slots.lock`, `labs/.instances.lock`) and a run's `lock`, with its record |
| `os-registration` | the `cyberctf://` handler: Linux `~/.local/share/applications/<exe>-handler.desktop` (every build but a wdio one), a package's desktop entry; Windows `HKCU\Software\Classes\cyberctf`, a `RunOnce` `CyberCTF` for a WSL restart                                                                                                                                                                                                     | an installed package's own entries (until it's removed)                                                                                |
| `keychain-item`   | service `org.cyberctf.desktop`, accounts `session`, `session.<n>`, `server:<host id>` (a release build; a debug or wdio build keeps them in files under its home)                                                                                                                                                                                                                                                                    | the player's sign-in and servers                                                                                                       |

The launcher's data folder holds by design its labs (`labs/<id>/`), Isothrum's run records
(`runs/cyberctf-<id>/record.json`, `events.jsonl`), the Terraform plugin cache
(`terraform-plugins/`), its SSH key (`ssh/id_ed25519`), its settings (`server.json`,
`storage.json`, `installed-tools.json`), the deploy logs and `~/CyberCTF/shared`; a desktop run
keeps them all in its own folder (`CYBERCTF_WDIO_ROOT`), removed at its end. On macOS the test
runs' webviews share one WebKit data store, `cyberctf-e2e-run`, by design.

### The network

| Kind            | What the stack makes                                                                                        | Kept by design |
| --------------- | ----------------------------------------------------------------------------------------------------------- | -------------- |
| `hosts-entry`   | none                                                                                                        | none           |
| `route`         | Docker's and libvirt's bridges' routes, going with their networks                                           | none           |
| `nft-table`     | Docker's and libvirt's tables (Linux)                                                                       | none           |
| `iptables-rule` | Docker's chains and rules for a lab network (`br-<id>`), going with it                                      | none           |
| `pf-anchor`     | none (macOS)                                                                                                | none           |
| `firewall-rule` | none of the launcher's; Windows asks to allow the app on its first listening socket (a rule named after it) | none           |
| `portproxy`     | a Hyper-V lab's published ports, `127.0.0.1:<port>` (`netsh interface portproxy`), deleted with its VM      | none           |

Neither the launcher nor Isothrum edits the hosts file, routes, pf, nftables, iptables or sysctl
on the machine: what segments a lab and blocks its egress runs inside it. These kinds catch a
change that would.

### Servers

| Kind               | What the stack makes on a Proxmox VE node or an ESXi host                                                                                        | Kept by design                                            |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------- |
| `proxmox-vm`       | `iso<slot>-<machine>`, `iso<slot>-isoloom-router`, `-isoloom-controller`, `iso<slot>-<env>` (a container lab in a VM), tagged `isoloom`, `<env>` | none                                                      |
| `proxmox-ct`       | a lab's containers, tagged the same                                                                                                              | none                                                      |
| `proxmox-sdn-zone` | a simple zone `iso<slot>`                                                                                                                        | none                                                      |
| `proxmox-sdn-vnet` | `i<slot>n<i>` a network, `i<slot>l<j>` a VLAN-carrying LAN                                                                                       | none                                                      |
| `proxmox-network`  | none (the bridge, `vmbr0` by default, is used)                                                                                                   | none                                                      |
| `proxmox-pool`     | `iso<slot>` (comment `Isoloom: <env>`)                                                                                                           | none                                                      |
| `proxmox-user`     | `isoloom`                                                                                                                                        | none                                                      |
| `proxmox-token`    | the launcher's `<user>!cyberctf-<8 hex>`, made when the node is set up                                                                           | that token, for the node's next lab                       |
| `proxmox-ipset`    | the firewall's `iso<slot>n<i>`                                                                                                                   | none                                                      |
| `proxmox-mapping`  | directory mappings `iso<slot>-<volume>`                                                                                                          | none (the folder they map is the player's)                |
| `proxmox-volume`   | disks, cloud images `iso<slot>-<os>[-arm64].img`, snippets `iso<slot>-<file>.yaml`, container templates `iso<slot>-<name>.tar`                   | none                                                      |
| `esxi-vm`          | `iso<slot>-<machine>` (Terraform), `<env>-<machine>` (Vagrant), the server self-test's `cyberctf-server-selftest-probe`                          | none                                                      |
| `esxi-vswitch`     | `iso<slot>-<net>` (no uplink), `iso<slot>-<nic>` (an outside network's)                                                                          | none                                                      |
| `esxi-portgroup`   | `iso<slot>-<net>`                                                                                                                                | none (Vagrant's use `ESXI_VIRTUAL_NETWORK`, the player's) |
| `esxi-file`        | a guest's folder on its datastore, `iso<slot>-<name>-disks`                                                                                      | none                                                      |

The launcher also appends its public key to the node's `~/.ssh/authorized_keys` when the node is
set up, kept for its next lab. In a cloud, every resource is tagged `isoloom-expires-at`, and the
launcher's reaper destroys an expired run; the audit doesn't reach clouds.

## Known gaps, from the code

What the code says will be left, until a run says otherwise:

- Isoloom's QEMU hub writes its pid and log files next to the Vagrantfile
  (`.isoloom/vagrant/.vagrant/isoloom-qemu-hubs/`), outside Isothrum's run folder; the log and the
  folder are never removed (`vagrant-dotfolder`), and a hub whose VM never got an id is never
  stopped (`process`).
- Isothrum's `project.tgz` in a run's folder has no removal.
- Clean-up of leftover VMs covers VirtualBox and Parallels only: a libvirt domain or network, a
  VMware VM or a Hyper-V switch left by an interrupted `vagrant destroy` stays.
- `tests/desktop/lib/owned.mjs` looks for a run's Vagrant machines under `app/labs`, while the
  runner keeps their state in `app/runs/cyberctf-<id>/vagrant`.
