# Labs on Isoloom (replacing `deploy/`)

Decided 2026-10-05: every lab describes itself with an `isoloom.yml` (https://www.isoloom.com,
https://github.com/isoloom/isoloom), and the launcher runs the files Isoloom generates. The
`deploy/` layer of each lab (and of the lab template) goes away.

## What maps where

| Today (`deploy/`) | With Isoloom |
| --- | --- |
| Container lab on the player's Docker (root `docker-compose.yml`) | `docker` target: `.isoloom/docker/compose.yml` |
| Container lab in a VM on this machine (`deploy/vagrant` lab host) | "Docker on one VM" output for Vagrant (to build) |
| Container lab on ESXi (`deploy/vagrant`, vagrant-vmware-esxi) | "Docker on one VM" output for Vagrant, ESXi provider (to build) |
| Container lab on a Proxmox server (`deploy/terraform/proxmox`) | "Docker on one VM" output for Proxmox (to build), or one VM per machine (`.isoloom/proxmox`, built) |
| Container lab on AWS, Azure, GCP, DigitalOcean, Linode, Oracle (`deploy/terraform/<cloud>`) | `cloud-docker` target: "Docker on one VM" output per cloud (to build) |
| VM lab (root `Vagrantfile`) | `vagrant` target: `.isoloom/vagrant/Vagrantfile` |
| Hosted lab (Vercel Sandbox runs the Compose file) | `hosted` target: the same `.isoloom/docker/compose.yml` |
| Ready file the launcher polls (`/var/lib/cyberctf/status`) | Isoloom's ready marker (`/var/lib/isoloom/ready`), on every VM output (to finish) |

Stays in the launcher (not Isoloom's business, by design): the Exegol attack box and the networks
it joins, launch tokens and other values passed as Isoloom `inputs`, the player's providers and
credentials, timed destroy and budgets.

## Order

1. Isoloom: the "Docker on one VM" output (Vagrant local and ESXi, Proxmox, each cloud), and the
   ready marker everywhere.
2. Launcher: depend on Isoloom's crates (`isoloom-spec`, `-plan`, `-targets`, `-runtime`; git dependencies, the repo is public). For a lab, read
   `isoloom.yml`; its derived targets decide the run-on choices; generate into the lab's
   `.isoloom/` and run with the existing runners (docker compose, vagrant, terraform).
3. Labs: an `isoloom.yml` per lab (starting with invoice-portal-api) and in the lab template;
   CI runs `isoloom check`. Then remove `deploy/` and the launcher code that reads it.
