# Server: labs on your own ESXi / Proxmox (and cloud)

The launcher can run a lab on the player's own hypervisor instead of this machine.
Hosts are added in the **Server** setup window; a lab's **Run on** choice picks the
host. VM labs launched from the website use the **default** host; Docker labs run here
unless a host is picked.

## Where things live

- Host profiles (non-secret): `<app data>/server.json`.
- Passwords: OS keychain, service `org.cyberctf.desktop`, account `server:<host id>`.
  Debug builds use `~/.cyberctf/dev-server-secrets.json` (0600) instead, as with the auth session.
- A lab started on a host gets `<lab dir>/.cyberctf-host` (the host id), so stop and
  status reach the same host. It survives a reinstall of the lab at a new commit.
- Terraform state: `<app data>/deployments/<lab id>/<target>/` (outside the lab folder).

Backend: `src-tauri/src/runtime/server.rs`. Commands: `server_list`, `server_save`,
`server_remove`, `server_set_default`, `server_test`, `server_open_setup`.

## How a lab reaches a host

Every lab comes from the **Lab-Template** (`Cyber-Courses/Lab-Template`): its
`docker-compose.yml` is the lab, and its `deploy/` layer runs that compose file on a
Debian "lab host" VM, configured by `deploy/ansible/site.yml` **inside** the VM.

| Lab | Host | Launcher runs |
| --- | --- | --- |
| Docker | ESXi | `vagrant up --provider vmware_esxi` in `deploy/vagrant` |
| Docker | Proxmox | `terraform apply` in `deploy/terraform/proxmox`, on macOS in the `hashicorp/terraform:1.9.8` container (pinned by digest in `runner.rs`) |
| VM (own Vagrantfile) | ESXi | `vagrant up --provider vmware_esxi` at the lab root |
| VM (own Vagrantfile) | Proxmox | not supported (no working Vagrant provider) |

Lab services have no host ports: the player attacks from the attack box, which the
launcher starts next to the lab on the lab host (`attackbox_image`, the Settings image).

### Vagrant (ESXi): environment

| Variable | Value |
| --- | --- |
| `CYBERCTF_REMOTE_PROVIDER`, `CYBERCTF_REMOTE_HOST` | `vmware_esxi`, hostname / IP |
| `CYBERCTF_ESXI_HOSTNAME`, `CYBERCTF_ESXI_HOSTPORT` | host, SSH port (22) |
| `CYBERCTF_ESXI_USERNAME`, `CYBERCTF_ESXI_PASSWORD` | e.g. `root`, password |
| `CYBERCTF_ESXI_DISK_STORE`, `CYBERCTF_ESXI_VIRTUAL_NETWORK` | optional datastore, port group |
| `CTF_API_URL`, `CTF_LAUNCH_TOKEN`, `CYBERCTF_ATTACKBOX_IMAGE` | evidence claim, attack box |

The Vagrantfile uses `esxi.esxi_password = "env:CYBERCTF_ESXI_PASSWORD"`.

### Terraform (Proxmox): `TF_VAR_*`, passed through the container environment

`proxmox_endpoint` (`https://host:8006/`), `proxmox_username` (`root@pam`),
`proxmox_password`, `proxmox_insecure` (the host's "Self-signed certificate" setting),
`proxmox_ssh_address` (the host as entered), `proxmox_node`, `proxmox_storage`,
`proxmox_bridge`; plus `lab_slug`, `lab_repository`, `lab_commit` (recorded at install),
`ctf_api_url`, `ctf_launch_token`, `attackbox_image`. The VM fetches the lab from GitHub
at that commit, so lab repositories must be public.

## Cloud (AWS)

An AWS account is stored like a host (provider `aws`): region in `host`, access key id
in `username`, secret access key in the keychain, optional instance type in `datastore`.
It lists on the **Cloud** page and appears in a Docker lab's Run on choice ("AWS, billed
to you"); it is never a default host, so a lab never lands on AWS implicitly.

The launcher runs `deploy/terraform/aws` with `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` /
`AWS_REGION` as raw container env (never Terraform variables), plus `region`,
`instance_type`, `ssh_public_key` and `allowed_cidr` (this machine's public IP/32 from
checkip.amazonaws.com, the only address allowed to SSH in). Stop runs `terraform destroy`.

**Auto-stop:** each AWS account has `auto_stop_hours` (default 4, 0 to 72, 0 = never). The
instance schedules its own poweroff at boot (systemd timer from cloud-init) and is set to
terminate on shutdown, so a forgotten lab stops billing even with this machine off. The
launcher records the expiry at apply and shows "Auto-stops at ..."; past it the lab shows
as stopped. The security group (free) stays until the next Start or Stop.
A reap that fails (expired credentials, a removed account, a destroy past its 30-minute limit)
is logged, kept beside the state (`reap-error`), shown on the lab's page and notified once.

**Where a destroy goes:** `run.json` beside the state keeps the placement the apply used (region,
project, compartment, Proxmox endpoint and node, the provider's `AWS_REGION` / `GOOGLE_PROJECT`;
never a secret), and Stop, the reaper and a leftover's destroy use it over the account's current
settings. While labs are deployed on a host, Settings refuses to change its provider, address or
region, port, node or the account labs are created in (credentials can still change).

**Provider plugins** are shared by every lab's runs in `<app data>/terraform-plugins`
(`TF_PLUGIN_CACHE_DIR`, one `init` writing it at a time); a successful destroy removes the
deployment's own `.terraform`. The Machine page's Downloads counts and cleans the cache.
The test is `sts get-caller-identity` in the `amazon/aws-cli` container.

## Attack box shell on remote labs

The attack box runs next to the lab on the lab host. "Open shell" opens the OS terminal on
`ssh -t <user>@<lab host> sudo docker exec -it attacker bash`:

- Terraform targets: the launcher's own key (`<app data>/ssh/id_ed25519`, made once with
  `ssh-keygen`), installed by cloud-init; user and address from the state outputs
  (`ssh_user`, `ip`). Own `known_hosts` next to the key.
- Vagrant targets (ESXi): `vagrant ssh-config` (Vagrant's key).

Verified 2026-10-03 on the test Proxmox: cloud-init installed the launcher key, and
`ssh debian@<lab host> sudo docker exec attacker ...` reached the attack box, which
resolves `web` and `database` on the lab network.

## Test

`server_test` checks the host without running anything on it:

- **ESXi**: TCP connect + SSH banner on the SSH port. The password is checked on first start.
- **Proxmox**: `POST /api2/json/access/ticket`, which verifies the credentials (accepting a
  self-signed certificate when the host is set to).

## Requirements on the host

- **ESXi**: SSH enabled; VMware `ovftool` on the launcher machine (vagrant-vmware-esxi).
- **Proxmox**: root SSH with the password (stock Proxmox allows it; Terraform uploads the
  cloud-init snippet over SSH), `snippets` + `iso` content on the `local` storage, a bridge
  with DHCP for the lab VM, KVM available (nested virtualization if Proxmox is itself a VM).

## Verified (2026-10-03)

- Lab host on a local VirtualBox VM via `deploy/vagrant` (Supplier Portal API): compose up,
  evidence claimed and placed, the attack box reads the invoice.
- `vagrant-vmware-esxi` 2.5.5 installs on Vagrant 2.4.9. `vagrant-proxmox` 0.0.10 (2016) does
  not (activesupport 4.0 vs Vagrant's i18n 1.14.7), hence Terraform for Proxmox.
- Terraform end to end on a test Proxmox VE 9 with nested KVM (`dev/test/server/proxmox-fusion`,
  on VMware Fusion): image download, snippet upload over SSH, VM, cloud-init, Ansible, compose
  healthy, the attack box reads `web`/`database`, and the remote "Open shell" key path works;
  `destroy` removes everything. Also verified on a Proxmox VE 8 VirtualBox host without nested
  KVM, where the lab VM needs `kvm: 0` + `cpu_type = x86-64-v2-AES` (plain `qemu64` lacks
  x86-64-v2, which `mysql:8.0` needs) and runs slowly.
- ESXi (`dev/test/server/esxi`, ESXi 8.0U3e on Fusion): VERIFIED end to end. The launcher's
  `deploy/vagrant` + `vmware_esxi` path uploads the lab VM with ovftool, it boots and gets an
  IP, Ansible installs Docker, the lab runs and the attack box reads the invoice. Nested lab
  VMs need Fusion's network-monitoring (promiscuous) permission approved once (a macOS admin
  prompt: Virtual Machine > Network Adapter, reconnect). Fusion's NAT DNS (172.16.1.2) is
  flaky under nested load; on the test host point the lab VM at public DNS if image pulls fail.
