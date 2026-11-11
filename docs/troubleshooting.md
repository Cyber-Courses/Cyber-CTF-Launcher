# Troubleshooting

Fixes for the problems people meet most. The launcher's own messages say what to do in most
cases; this page adds the context. When nothing here helps, open an issue with the diagnostics and
logs ([SUPPORT.md](../SUPPORT.md#what-to-send-with-a-bug-report)).

**Logs**: Settings > About > Log files > **Open folder**. `cyberctf.log` is the app's,
`deploy-<lab>.log` each lab's deploys. The folders are listed in the
[administrator guide](admin-guide.md#files-and-data).

## Signing in

| Problem                                           | What to do                                                                                                                                                                     |
| ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| No browser opens                                  | Use **Copy the sign-in link** and paste it into any browser on this machine. Or set a default browser for your account.                                                        |
| The browser can't be used (remote desktop, kiosk) | Use **Use a code instead** and enter the code on cyberauth.co from another device.                                                                                             |
| "no free login callback port (47290-47292)"       | Another program holds those three local ports. Close it, or use **Use a code instead**.                                                                                        |
| "Cyber CTF can't be reached"                      | The launcher can't reach `www.cyberauth.co` or `cyberbackend.com`. Check the connection; behind a proxy, see [Proxy](admin-guide.md#proxy-and-custom-certificate-authorities). |
| Signed out at every start (Linux)                 | The session is kept in the Secret Service (GNOME Keyring, KWallet). Make sure one runs and is unlocked in your desktop session.                                                |

## The lab catalogue

"Can't reach the lab catalogue": the launcher can't reach `cyberbackend.com`. Check the connection
and the proxy. A lab you already installed and started before can still start: the launcher offers
**Start offline** (your progress isn't saved until you start it online again).

## Docker and container labs

| Message or problem                                               | What to do                                                                                                                                     |
| ---------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| "Docker isn't running"                                           | Start your container engine (Docker Desktop, OrbStack…, or `sudo systemctl start docker` on Linux), then start the lab again.                  |
| "Docker is running, but your account can't use it yet" (Linux)   | `sudo usermod -aG docker $USER`, then log out and back in. Membership of `docker` is equivalent to root on the machine.                        |
| "Docker is running, but your account can't use it yet" (Windows) | In a terminal run as administrator: `net localgroup docker-users "<you>" /add`, then sign out and back in.                                     |
| "Host port … is already in use"                                  | Another lab or program uses a port the lab publishes. Stop it, or set Settings > Labs > Lab ports to **Random** (each service on a free port). |
| Docker Desktop won't start on Windows                            | It needs WSL 2: run the **Enable virtualization (WSL 2)** step on the Machine page, and restart Windows when asked.                            |
| A large download fails or the disk fills up                      | Settings > Maintenance > Cleanup removes the images and VM boxes the launcher downloaded, and leftover labs.                                   |

## VM labs

- **Hardware virtualization** (Intel VT-x or AMD-V) must be on in the computer's firmware settings.
- **Hyper-V** (Windows): only administrators and members of Hyper-V Administrators may manage VMs.
  Without that, each batch of VM commands asks for administrator rights. Settings > Labs >
  Hypervisor > **Allow my account** adds you to the group, once; it applies from your next Windows
  sign-in.
- **Which virtual switch** (Hyper-V): when Vagrant would ask which switch to use, set the
  environment variable `ISOLOOM_HYPERV_SWITCH` to the switch's name (`Get-VMSwitch` lists them), or
  keep a single virtual switch.
- **Apple silicon**: a lab whose VMs exist only for x86 (pfSense, for instance) runs under QEMU
  emulation, several times slower. Run such labs on an x86 server (Proxmox, ESXi) instead.
- **VirtualBox on Windows on Arm** doesn't exist: use Hyper-V, a server or a cloud account.
- **A start was interrupted**: the lab page offers **Stop & clean up**; then start it again.

## Servers and cloud accounts

- **Test** on the Servers or Cloud page checks the host or account before a lab runs
  there, and says what is missing. [Running labs](running-labs.md#requirements-on-a-server) lists
  what Proxmox and ESXi need.
- **A cloud sign-in ran out**: before changing a lab in AWS, Azure, Google Cloud or Oracle Cloud,
  the launcher checks that cloud's CLI sign-in and, when it ran out, says how to sign in again
  (`aws login`, `az login`, …). It changes nothing until you do.
- **"Couldn't confirm your public IP"**: a cloud lab's firewall opens only to this machine's public
  address, which at least two lookup services must agree on. A VPN or a second internet
  connection can make them disagree; so can a network that blocks them
  ([the services](admin-guide.md#network)).
- **"Auto-stop couldn't destroy …"**: the message says why. If the launcher can no longer reach
  the lab's account or files, remove the lab's resources in the cloud console so they stop
  billing.
- **"Lab … couldn't be handed over to the Isothrum runner"**: a cloud or Proxmox lab an older
  version of the launcher started with its own Terraform, which the runner takes over at start.
  Until it does, the launcher can't remove that lab: Stop and the auto-stop leave it running, and
  a cloud lab keeps billing. Delete its resources in the cloud console (or its VMs on the Proxmox
  server); the launcher tries the hand-over again every few minutes while it is open.

## Installing tools

- **macOS**: the launcher installs with Homebrew; install it first from <https://brew.sh>. A cask
  (Docker Desktop, Vagrant, VirtualBox, UTM) opens its own installer: finish it, then go back to
  the launcher.
- **Windows**: installs go through winget (App Installer, from the Microsoft Store). If winget is
  missing, install App Installer, then try again.
- **Linux**: installs run as root through a polkit prompt (`pkexec`); the user needs the right to
  administer the machine.

## Updates

"Couldn't reach the update server": the launcher can't reach `github.com`. It keeps working on the
installed version. An update waits until no lab is starting, stopping, pausing or being provisioned, because
installing restarts the app and would cut that operation off.
