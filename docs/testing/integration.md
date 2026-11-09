# Integration tests

What only the real tools show: a lab on real Docker, Vagrant, hypervisors and clouds, end to end.
One command runs it, one verdict per scenario:

```sh
scripts/integration-tests.sh                       # the local tier (pnpm test:integration)
TIERS="local aws" scripts/integration-tests.sh     # and the AWS tier: billed while it runs, ~20 minutes
TIERS=server scripts/integration-tests.sh          # a lab on a Proxmox / ESXi host
ONLY="box-docker box-vm" scripts/integration-tests.sh
```

Each scenario is an opt-in Rust test (`#[ignore]`, so `cargo test` never runs it), given the
knobs it needs; its output is in `.integration-tests/<scenario>.log`, its checks (`ok:` / `FAILED:`) on
the screen. The labs are Isoloom's examples (`ISOLOOM`, default `../isoloom`). Run it before a
release that touches running labs, after a change to Isoloom or Isothrum, and after any field bug
in the tiers below (then add the scenario that would have caught it).

## Local tier

Docker, and Vagrant with QEMU (`VM_PROVIDER`) and, on a Mac, UTM for a container lab in a VM
(`DOCKER_VM_PROVIDER`). About 30 minutes, most of it Kali's install on the VM form.

| Scenario               | Proves                                                                                                                                                      | Guards against (found)                                                                                                                                                                                                                                                    |
| ---------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `box-docker`           | the attack box added to a running Docker lab as a machine of it, reaching `web`, called `attackbox`, the shared folder seen both ways at `/shared`, removed | the box is the lab's: regenerated with it, the run updated, taken out by `--remove-orphans`; on an Arm Mac it runs Kali's arm64 image natively (it was emulated amd64)                                                                                                    |
| `box-docker-vm`        | the same, the box a container inside the lab's VM, its shell through the VM                                                                                 | the console into a service inside a VM (`vagrant ssh … docker exec` by its Compose label); the box native on the Arm helper VM                                                                                                                                            |
| `box-vm`               | the same among a VM lab's, a Kali VM with `kali-linux-headless`                                                                                             | on an Arm Mac Kali has no Vagrant box: a native Debian made Kali. Its set-up keeps the VM's kernel (Kali's didn't fit the EFI partition), takes Kali's archive only, pinned (Debian's newer `tzdata` blocked Kali's), and lets renamed libraries overwrite (the t64 move) |
| `docker-lab`           | a Docker lab's whole life: up, status, park and resume, checks, down, nothing left                                                                          |                                                                                                                                                                                                                                                                           |
| `docker-lab-segmented` | the same with several networks and no published port                                                                                                        |                                                                                                                                                                                                                                                                           |
| `docker-vm-lab`        | a container lab in one local VM, up to down, no VM left                                                                                                     | UTM's transient `-600` scripting error; macOS 27's port check; amd64 images on an Arm helper VM                                                                                                                                                                           |
| `vm-lab`               | a VM lab up to down on the hypervisor                                                                                                                       |                                                                                                                                                                                                                                                                           |
| `per-machine`          | a container lab run as one VM per machine, the page's operations following that form                                                                        | the example's MariaDB refusing root over TCP (its seed's way in)                                                                                                                                                                                                          |
| `launch-commands`      | a local lab launched and torn down through the app's own commands                                                                                           |                                                                                                                                                                                                                                                                           |
| `replace-docker`       | a running Docker lab's folder is not replaced: an update and a re-import are refused while it runs, and go through once it is stopped                       | the update and the import asked the lab folder (Vagrant's `.vagrant`, `deployments/`), where the runner keeps nothing, so a run's files were swapped out from under it                                                                                                    |
| `replace-docker-vm`    | the same for a container lab in a local VM                                                                                                                  |                                                                                                                                                                                                                                                                           |
| `replace-vm`           | the same for a VM lab                                                                                                                                       |                                                                                                                                                                                                                                                                           |

## AWS tier

The AWS CLI signed in (`aws login`), Terraform, `AWS_REGION` (default `eu-west-3`). The lab
runs in that account through the launcher's own cloud start and stop, then the account is
checked for anything of it left (instances, networks, key pairs).

| Scenario         | Proves                                                                                            | Guards against (found)                                                                                                                                                                              |
| ---------------- | ------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `box-aws-docker` | cloud-docker: the box a container in the lab's cloud VM, its shell over SSH into it, nothing left | the box wrapped in a VM of its own; a lab left behind                                                                                                                                               |
| `box-aws-vm`     | cloud-vm: the box a Kali instance among the lab's, added by updating the run, nothing left        | Debian booted instead of Kali (the Marketplace AMI), a 20 GB disk under Kali's 25, the box as a target, the host name `kali`, set-up run again on every update, the instances replaced by an update |

A sign-in that runs out mid-run left a lab's network behind once (an empty VPC, its firewall and
key): the launcher now asks the cloud's CLI before every change and refuses, saying to sign in
again (`runtime/isothrum/cloud.rs`, unit-tested); the runner refuses to start the AWS tier
signed out.

## Server tier

A Proxmox or an ESXi host (`infra/test-servers/` makes a throwaway one), named by its knobs: the lab
runs on it through the runner, as the launcher drives it, then nothing of it is left. Without a
host's knobs its scenario is skipped, saying which ones it needs.

| Scenario      | Proves                                                                   |
| ------------- | ------------------------------------------------------------------------ |
| `proxmox-lab` | a VM lab on Proxmox: up, status, checks over SSH through the node, down  |
| `esxi-lab`    | a VM lab on ESXi through Vagrant: up, status, checks, provisioning, down |

## Not here yet

- The attack box on a server: `box-*` on Proxmox, once a test host is back (a lab on ESXi has
  no attack box).
- The shell as a person sees it: a console whose prompt went to its error output and showed
  nothing after Kali's MOTD (now a unit test in Isothrum: the shell is chosen first, then started
  with nothing redirected), and a shell offered before the box finished installing (the side
  rail waits for the box to be idle).
- Other clouds: the same test with `CYBERCTF_TEST_BOX_FORM` and an account of theirs is the next
  step; their sign-in check is unit-tested.
