# Administrator guide

For IT and security teams deploying the Cyber CTF launcher: what it installs and changes, what it
connects to, where it keeps data, what it sends, and how to install and remove it without user
interaction. The behaviour described is that of the code in this repository; file paths use the
app identifier `org.cyberctf.desktop`.

In short:

- The launcher is a desktop app that runs **deliberately vulnerable training labs** on the user's
  own machine (Docker, local VMs), on a Proxmox or ESXi server, or in the user's own cloud
  account. It drives Docker, Vagrant, Terraform, the hypervisors and the cloud CLIs; it does not
  bundle them.
- It needs **Cyber CTF's online services** (sign-in, the lab catalogue, starting a lab). It cannot
  run air-gapped.
- The app installs **per user** (NSIS, macOS, AppImage) or **per machine** (MSI, `.deb`). The
  tools labs need are installed on demand, each through the OS's own elevation prompt.
- Machine-wide settings an administrator can enforce are in the
  [policy reference](reference/policy.md).

## Contents

- [Install and deploy](#install-and-deploy)
- [Updates](#updates)
- [Verify a download](#verify-a-download)
- [Network](#network)
- [Proxy and custom certificate authorities](#proxy-and-custom-certificate-authorities)
- [Offline use](#offline-use)
- [Files and data](#files-and-data)
- [Changes to the machine](#changes-to-the-machine)
- [Privacy and what is sent](#privacy-and-what-is-sent)
- [Logs and diagnostics](#logs-and-diagnostics)
- [Uninstall](#uninstall)

## Install and deploy

Every release is on [GitHub Releases](https://github.com/Cyber-Courses/Cyber-CTF-Launcher/releases).
`<version>` below is the release's version, without the `v`.

| Platform              | Package                                    | Scope       | Silent install                                                |
| --------------------- | ------------------------------------------ | ----------- | ------------------------------------------------------------- |
| Windows x64           | `Cyber.CTF_<version>_x64_en-US.msi`        | per machine | `msiexec /i Cyber.CTF_<version>_x64_en-US.msi /qn /norestart` |
| Windows x64           | `Cyber.CTF_<version>_x64-setup.exe` (NSIS) | per user    | `Cyber.CTF_<version>_x64-setup.exe /S`                        |
| Windows Arm64         | `Cyber.CTF_<version>_arm64-setup.exe`      | per user    | `Cyber.CTF_<version>_arm64-setup.exe /S`                      |
| Windows x64 and Arm64 | Microsoft Store (product `9P9L4WZ0N8ZJ`)   | per user    | `winget install --id 9P9L4WZ0N8ZJ --source msstore`           |
| macOS (universal)     | `Cyber.CTF_<version>_universal.dmg`        | per machine | copy `Cyber CTF.app` to `/Applications`, or Homebrew (below)  |
| Linux x64 and arm64   | `.deb`, or the apt repository              | per machine | `apt-get install -y cyber-ctf` (repository below)             |
| Linux x64 and arm64   | `.AppImage`                                | per user    | none: a single executable file                                |

**Windows.**

- The **MSI** is the package for Intune, Configuration Manager or Group Policy software
  installation: it installs for all users under `C:\Program Files\Cyber CTF`. `INSTALLDIR=<path>`
  chooses another folder. Remove it with `msiexec /x <msi or product code> /qn`. It is built for
  x64 only.
- The **NSIS** installer installs for the current user, without administrator rights, under
  `%LOCALAPPDATA%\Cyber CTF`. `/S` is silent; `/D=<path>` (last, unquoted) chooses the folder.
- The **Store** package is signed by Microsoft and updated by the Store; it can be deployed through
  Intune's Microsoft Store app (new) type.
- All of them need the Microsoft Edge WebView2 Runtime, which Windows 11 includes. When it is
  missing, the MSI and NSIS installers download Microsoft's WebView2 bootstrapper (from
  `go.microsoft.com`); on machines without internet access, deploy the WebView2 Runtime first.
- Install one package type per machine: the MSI and the NSIS installer are separate installs.

**macOS.** Homebrew:

```bash
brew tap Cyber-Courses/homebrew-tap
brew trust --tap cyber-courses/tap   # Homebrew 7 loads casks from trusted taps only
brew install --cask cyberctf
```

The app is signed with a Developer ID (Cyber Courses, team `9SAJ3QR6W9`) and notarized. For a
managed deployment, copy the app from the DMG to `/Applications` with your MDM's script or package
tooling.

**Linux.** The apt repository (Debian and Ubuntu, amd64 and arm64) is signed with the key
`B4B6 770F 5BED 6F3E 91BA C67E EB61 45F4 8E6D 7C1C` ("Cyber CTF apt <apt@cyberctf.org>"):

```bash
sudo curl -fsSL https://cyber-courses.github.io/apt/cyberctf-apt.asc -o /usr/share/keyrings/cyberctf.asc
gpg --show-keys /usr/share/keyrings/cyberctf.asc   # check the fingerprint above
echo "deb [signed-by=/usr/share/keyrings/cyberctf.asc] https://cyber-courses.github.io/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/cyberctf.list
sudo apt-get update && sudo apt-get install -y cyber-ctf
```

The package depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0` and `pkexec` (or `policykit-1`). The
builds target glibc 2.35 (Ubuntu 22.04) and later.

**Admin policy.** Settings that apply to every user of a machine, and that users cannot change,
are described in the [policy reference](reference/policy.md).

## Updates

| Installed from                               | Updated by                         |
| -------------------------------------------- | ---------------------------------- |
| GitHub installers (MSI, NSIS, DMG, AppImage) | the launcher's own updater (below) |
| Homebrew                                     | `brew upgrade --cask cyberctf`     |
| .deb (apt repository or GitHub)              | `apt-get upgrade`                  |
| Microsoft Store                              | the Store                          |

Installed from Homebrew, a `.deb` or the Store, the launcher's own updater is off: Settings >
About says where updates come from.

The launcher's own updater checks
`https://github.com/Cyber-Courses/Cyber-CTF-Launcher/releases/latest/download/latest.json` when it
starts and, when a newer version exists, shows a banner. Nothing is installed until the user
clicks **Update** (or **Install update** in Settings > About). It installs only a package signed
with the launcher's update key (minisign; the public key is in `src-tauri/tauri.conf.json`), and
it waits while any lab operation runs (a start, a stop, a pause or a provisioning run, in the app
or in a deploy worker), because installing quits every Cyber CTF process. On Windows the update
runs the same kind of installer as the installed one (NSIS or MSI); the MSI needs administrator
rights. For a managed fleet, deploy new
versions with your own tooling and see the [policy reference](reference/policy.md) for turning the
updater off.

Versions follow semantic versioning, and only the latest release gets security fixes
([versioning and support policy](reference/versioning.md)).

## Verify a download

Each release carries, beside the installers:

- `SHA256SUMS`: the SHA-256 of every file of the release.
  `sha256sum --ignore-missing -c SHA256SUMS` (Linux), `shasum -a 256 --ignore-missing -c SHA256SUMS`
  (macOS), `Get-FileHash <file>` (Windows).
- A build provenance attestation of every file, signed through Sigstore and kept by GitHub: it
  proves the file was built by this repository's release workflow from a given commit.
  `gh attestation verify <file> --repo Cyber-Courses/Cyber-CTF-Launcher`.
- `cyberctf-rust.cdx.json` and `cyberctf-npm.cdx.json`: CycloneDX SBOMs of the Rust crates and the
  npm packages.
- `THIRD_PARTY_NOTICES`: the licences of the third-party software in the app (also inside the
  installed app).

Signatures: the macOS app and DMG are signed and notarized (`spctl -a -vvv "/Applications/Cyber CTF.app"`).
Windows installers are Authenticode-signed from the release after 1.8.9
(`Get-AuthenticodeSignature <file>`); up to 1.8.9 they were not. The `.sig` files are the in-app
updater's signatures.

## Network

The launcher needs outbound HTTPS (TCP 443). It listens on no external interface.

### Endpoints the launcher itself connects to

| Host                                                                     | Why                                                                    | When                                         |
| ------------------------------------------------------------------------ | ---------------------------------------------------------------------- | -------------------------------------------- |
| `www.cyberauth.co`                                                       | sign-in (OAuth 2.0 with PKCE, device code), token refresh              | sign-in, then about once per token lifetime  |
| `cyberbackend.com`                                                       | the lab catalogue, starting a lab, progress; the website agent (below) | while the app runs and the user is signed in |
| `codeload.github.com`                                                    | a lab's source, at the commit the catalogue pins                       | installing or updating a lab                 |
| `raw.githubusercontent.com`                                              | a lab's `isoloom.yml`, read before the lab is installed                | opening or starting a lab                    |
| `hub.docker.com`                                                         | the download size of an attack-box image (Docker Hub's API)            | choosing the attack box image                |
| `github.com`, `release-assets.githubusercontent.com`                     | the updater's `latest.json` and update packages                        | at start-up; when the user updates           |
| `checkip.amazonaws.com`, `api.ipify.org`, `ifconfig.me`, `icanhazip.com` | this machine's public IPv4 address                                     | starting a lab in a cloud account only       |
| `api.digitalocean.com`, `api.linode.com`                                 | checking a DigitalOcean or Linode token                                | "Test" on such an account                    |
| the user's Proxmox or ESXi host                                          | the server's API, SSH                                                  | labs on that server                          |

Sign-in happens in the user's default browser, which must reach `www.cyberauth.co` (and the
identity provider the user signs in with there). The public-IP lookup asks all four services and
needs two to agree: a cloud lab's firewall is opened to that address only. Behind a proxy or NAT,
that is the organisation's egress address.

The app window shows the account's profile picture when the identity provider gives one on
`avatars.githubusercontent.com` (the only remote image the window may load; any other shows the
account's initials); everything else in the window is bundled with the app.

The endpoints of Cyber CTF's services are fixed in a released build; only development builds read
them from the environment ([configuration reference](reference/configuration.md)).

### Traffic of the tools the launcher runs

Labs are run by external tools, which connect on their own, with their own proxy and certificate
settings:

| Tool                     | Typical hosts                                                                                                                                                                                 |
| ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Docker                   | Docker Hub (`registry-1.docker.io`, `auth.docker.io`, `production.cloudflare.docker.com`) and the registries a lab's images name; the attack box image is `cyberctf/attack-box` on Docker Hub |
| Vagrant                  | Vagrant Cloud (`vagrantcloud.com`) and where each box is hosted; `rubygems.org` for plugins                                                                                                   |
| Terraform                | `registry.terraform.io`, `releases.hashicorp.com` (providers)                                                                                                                                 |
| Vagrant VMware Utility   | `releases.hashicorp.com` (macOS and Windows), `apt.releases.hashicorp.com` (Linux): installed once, from the Machine page                                                                     |
| Cloud CLIs and Terraform | the cloud provider's APIs (AWS, Azure, Google Cloud, DigitalOcean, Linode, Oracle Cloud)                                                                                                      |

The lab machines themselves download too: a Kali attack VM fetches its packages from
`archive.kali.org` and `http.kali.org` (Kali's mirror redirector, plain HTTP, signed packages;
on a Debian VM made Kali, Kali's archive keyring is checked against its pinned fingerprints first); a
container lab run inside a VM installs Docker from `download.docker.com`; server and cloud labs
download OS images (`cloud-images.ubuntu.com`, `cloud.debian.org`, `repo.almalinux.org`). Labs can
reach whatever their own definitions name.

### Installing tools

When the user installs a tool from the Machine page or the first-run setup, the launcher runs the
OS's package manager or the vendor's installer:

| OS      | How                                                                                                                                                                                                                                        | Hosts                                                                                                                                                                                                                                                                                                                            |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| macOS   | Homebrew formulae and casks (`docker-desktop`, `vagrant`, `virtualbox`, `utm`, `qemu`, `hashicorp/tap/terraform`, `awscli`, `azure/azure-cli` tap's `azure-cli-preview`, `google-cloud-sdk`)                                               | Homebrew (`github.com`, `ghcr.io`, `formulae.brew.sh`) and each cask's vendor download host                                                                                                                                                                                                                                      |
| Windows | `winget install` (`Docker.DockerDesktop`, `Hashicorp.Vagrant`, `Hashicorp.Terraform`, `Oracle.VirtualBox`, `SoftwareFreedomConservancy.QEMU`, `Amazon.AWSCLI`, `Microsoft.AzureCLI`, `Google.CloudSDK`); `wsl --install --no-distribution` | winget's sources and each package's vendor download host                                                                                                                                                                                                                                                                         |
| Linux   | apt, through `pkexec`                                                                                                                                                                                                                      | the distribution's mirrors; `download.docker.com`; `apt.releases.hashicorp.com`; `download.virtualbox.org`, `www.virtualbox.org`; `awscli.amazonaws.com`; `packages.microsoft.com` (Azure CLI); `packages.cloud.google.com` (Google Cloud CLI). Every repository's key is pinned by fingerprint; no vendor install script is run |

On Linux every vendor repository's key (Docker's, HashiCorp's, VirtualBox's, Microsoft's for the
Azure CLI, Google's for the Google Cloud CLI) is checked against a fingerprint pinned in the app,
and the AWS CLI download against AWS's signing key, before anything is installed.

### Local ports

| Port                                | Bound to    | What                                                                                                                                                      |
| ----------------------------------- | ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 47290, else 47291, else 47292 (TCP) | `127.0.0.1` | the sign-in redirect (RFC 8252), only while a sign-in waits (at most 5 minutes)                                                                           |
| a free port chosen by the OS (TCP)  | `127.0.0.1` | after a website launch, a probe the website reads to check the lab runs on the same machine as the browser (one answer, at most 10 minutes)               |
| free ports chosen by the OS (TCP)   | `127.0.0.1` | relays to an ESXi host on the local network, on macOS, while a lab on it runs                                                                             |
| a free port chosen by the OS (TCP)  | `127.0.0.1` | a machine's screen window's bridge, while the window is open: one viewer, the app's own page, with a one-time token                                       |
| free ports chosen by the OS (TCP)   | `127.0.0.1` | a lab VM's graphical console (libvirt, QEMU, VMware, VirtualBox), or an SSH tunnel to a cloud Windows VM's Remote Desktop while its screen window is open |

Labs publish their own ports on the machine (Docker port mappings, VM port forwards), as their
definitions say.

## Proxy and custom certificate authorities

**Proxy.** The launcher's own HTTPS connections use, in this order: the `HTTPS_PROXY`,
`HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY` environment variables (upper or lower case), then the
system proxy settings on macOS (Network settings) and Windows (Internet Options). Proxy
auto-configuration (PAC, WPAD) is not read, and a proxy that needs NTLM or Kerberos authentication
is not supported; basic credentials can be given in the proxy URL. The tools the launcher runs
inherit its environment and follow their own proxy rules. On macOS, an app opened from the Dock
or Finder has no proxy variables, so when none is set the launcher puts the system proxy
(`scutil --proxy`, with its exceptions plus loopback in `NO_PROXY`) into the environment at
start, and the tools it runs get it; a PAC file can't be translated, and the log says so. On
Linux, tool installs run through `pkexec`, which clears the environment: the launcher passes the
proxy variables to it explicitly. Configure the tools' own proxy settings (Docker's daemon,
Homebrew, winget) as their vendors document where those don't read the environment.

**Certificate authorities.** The launcher verifies TLS against the operating system's trust store
(the macOS keychains, the Windows certificate store, and the system CA bundle on Linux). A
TLS-inspecting proxy works once its root CA is trusted by the OS. The tools keep their own
behaviour, as their vendors document it: Vagrant, for instance, ships its own CA bundle and needs
the corporate CA added to it (or `SSL_CERT_FILE` set).

## Offline use

The launcher cannot run air-gapped:

| Needs Cyber CTF's services (`cyberauth.co`, `cyberbackend.com`)                                     | Works without them                                                                                                                                         |
| --------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Signing in; browsing the catalogue; starting a lab for the first time; saving progress and evidence | Starting a lab already installed and started online before, after the user confirms "Start offline" (no progress or evidence is saved; not for cloud labs) |
| Website-started launches                                                                            | The machine checks, the Machine page, Settings                                                                                                             |
|                                                                                                     | Importing a lab from a local folder (an unpublished lab)                                                                                                   |

Even offline, a lab still needs its container images or VM boxes, which Docker and Vagrant
download the first time.

## Files and data

| What                              | macOS                                                         | Windows                                    | Linux                                      |
| --------------------------------- | ------------------------------------------------------------- | ------------------------------------------ | ------------------------------------------ |
| App data                          | `~/Library/Application Support/org.cyberctf.desktop`          | `%APPDATA%\org.cyberctf.desktop`           | `~/.local/share/org.cyberctf.desktop`      |
| Logs                              | `~/Library/Logs/org.cyberctf.desktop`                         | `%LOCALAPPDATA%\org.cyberctf.desktop\logs` | `~/.local/share/org.cyberctf.desktop/logs` |
| Cache (self-test scratch)         | `~/Library/Caches/org.cyberctf.desktop`                       | `%LOCALAPPDATA%\org.cyberctf.desktop`      | `~/.cache/org.cyberctf.desktop`            |
| Web view data                     | `~/Library/WebKit/org.cyberctf.desktop`                       | `%LOCALAPPDATA%\org.cyberctf.desktop`      |                                            |
| Folder shared with the attack box | `~/CyberCTF/shared` (or the folder chosen in Settings > Labs) | same                                       | same                                       |

On macOS and Linux the app data folder is made readable by its user only (mode 0700); on Windows
the per-user profile folder keeps other accounts out. On Windows it is under the roaming profile
(`%APPDATA%`) and can grow by gigabytes (labs, run state): exclude
`%APPDATA%\org.cyberctf.desktop` from roaming profile synchronisation.

In the app data folder:

| Path                                   | Holds                                                                                                                                                                                               |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `labs/<lab id>/`                       | each installed lab: its source (up to 512 MB downloaded, 4 GB unpacked) and the files generated to run it (Compose, Vagrant, Terraform)                                                             |
| `runs/<run>/`                          | the state of each running lab, kept by the Isothrum runner: its record, events, and for a server or cloud lab its **Terraform state**, which holds the lab's outputs, secrets included              |
| `server.json`                          | the user's servers and cloud accounts (names, addresses, regions; no secrets)                                                                                                                       |
| `ssh/id_ed25519`                       | the launcher's own SSH key (no passphrase), put on the server and cloud machines it creates                                                                                                         |
| `install-id`                           | a random identifier of this installation (see [Privacy](#privacy-and-what-is-sent))                                                                                                                 |
| `offline/`                             | which lab, runtime, repository and commit each lab last started from, for an offline start (no tokens)                                                                                              |
| `deploys/`                             | the jobs of the deploy workers (a deploy keeps running if the window closes), each with its output (`<lab>.log`, the run before kept as `<lab>.previous.log`); a job carries the lab's launch token |
| `storage.json`, `installed-tools.json` | the images, boxes and tools the launcher downloaded or installed, so Settings > Maintenance removes only those                                                                                      |
| `terraform-plugins/`                   | an older Terraform provider cache, cleanable from the Storage page                                                                                                                                  |
| `tools/ovftool/`                       | VMware's OVF Tool, when the user placed a download of it from the ESXi setup ("Use a downloaded OVF Tool…"): a copy of the unpacked folder, on the app's `PATH`                                     |

**Terraform state is the record of what a server or cloud lab created.** While such a lab runs,
deleting `runs/` (or the whole app data folder) leaves its machines running, and billing in a cloud
account, with nothing left to destroy them: stop labs first.

**Keychain.** Secrets are kept in the OS credential store under the service `org.cyberctf.desktop`:
the sign-in session (`session`, and `session.1`, `session.2`… when it is long) and one entry per
server or cloud account with a stored password or key (`server:<id>`). On Windows the Credential
Manager shows them as generic credentials named `session.org.cyberctf.desktop`,
`server:<id>.org.cyberctf.desktop`; on Linux they are in the Secret Service (GNOME Keyring,
KWallet), which must be running for the user to stay signed in. Tokens never reach the app's web
view or a file.

Outside these folders, the tools keep their own data: Docker images and volumes, Vagrant boxes
(`~/.vagrant.d`), the VMs' disks (the hypervisor's folder), and the cloud CLIs' credentials in
their usual places.

## Changes to the machine

Everything below happens only when the user takes the action, and each step that needs
administrator rights asks through the OS's own prompt (UAC, macOS's installer, polkit). The
launcher never asks for or stores a password itself.

**All platforms.**

- Installing the app registers the `cyberctf://` URL scheme, which the website uses to open a lab
  in the launcher. On Linux the app registers it for the user at start
  (`~/.local/share/applications/cyberctf-desktop-handler.desktop`).
- Tools installed from the Machine page (Docker, Vagrant, Terraform, a hypervisor, a cloud CLI) are
  ordinary installs of those products, recorded in `installed-tools.json` so that Settings >
  Maintenance can remove them again.
- Vagrant plugins a lab needs (`vagrant-*`) are installed for the user with `vagrant plugin install`.
- VMware Fusion and Workstation also need HashiCorp's **Vagrant VMware Utility**, a system service
  the `vagrant-vmware-desktop` plugin talks to on `127.0.0.1:9922`. The Machine page installs it
  from HashiCorp's own package (`releases.hashicorp.com/vagrant-vmware-utility/1.0.24/`, its
  SHA-256 pinned in the launcher), with an administrator prompt: on macOS the `.pkg` (checked as
  signed by HashiCorp's Developer ID, team `D38WU7D763`, and notarized) puts it in
  `/opt/vagrant-vmware-desktop` with a launchd service; on Windows the `.msi` (checked as
  Authenticode-signed by HashiCorp, Inc.) installs the `vagrant-vmware-utility` service; on Linux
  the `vagrant-vmware-utility` package comes from HashiCorp's apt repository and runs as a systemd
  service. It is refused under `DisableToolInstalls`.
- **Use a downloaded OVF Tool…** (the ESXi setup) copies the folder the user picks into
  `tools/ovftool/` in the app data folder, once it holds VMware's OVF Tool; on macOS only once the
  program and its libraries are verified as signed by VMware's Developer ID (team `EG7KH642X6`),
  and it then removes `com.apple.quarantine` from that copy, not from the download. It is refused
  under `DisableToolInstalls`. Details: [running labs](running-labs.md#the-ovf-tool-for-esxi).

**Windows.**

- **Turn on WSL 2** (needed by Docker Desktop) runs `wsl --install --no-distribution` elevated,
  which turns on the WSL and Virtual Machine Platform features; the launcher then offers to
  restart Windows and adds a one-time `RunOnce` entry under `HKCU` to reopen itself after it.
- **Allow my account** (Settings > Labs > Hypervisor) adds the user to the local
  **Hyper-V Administrators** group (SID `S-1-5-32-578`), which lets them manage every VM on the
  machine. Without it, each batch of Hyper-V commands asks for administrator rights.
- A Docker Desktop install may require the user to be in the `docker-users` group; the launcher
  says so but does not add them.

**Linux.**

- **Installing Docker adds the user to the `docker` group** (`usermod -aG docker <user>`). Members
  of `docker` can control the Docker daemon, which runs as root: **this is equivalent to root
  access** on the machine. Install Docker yourself (rootless, or without the group) where that is
  not acceptable.
- Installing Docker (where Docker's repository serves the release), Terraform, VirtualBox, the
  Azure CLI or the Google Cloud CLI adds the vendor's apt repository, for this machine's architecture, and its key, checked against a
  pinned fingerprint: `/etc/apt/sources.list.d/<name>.list` with the key
  `/etc/apt/keyrings/<name>.gpg`, `<name>` being `docker`, `hashicorp`, `virtualbox`, `azure-cli`
  or `google-cloud-sdk`. (Earlier versions put some of the keys under `/usr/share/keyrings/`.)
  Removing a tool from Settings > Maintenance leaves its repository and key in place.
- On Ubuntu, Debian, Raspbian and the distributions built on them, Docker comes from Docker's
  repository for the Ubuntu or Debian release the system is built on. Only where that repository
  answers that it has no packages for the release (an HTTP 404 for its `Release` file: Kali's
  rolling one, Devuan's, a release newer than Docker serves yet) does the launcher install the
  distribution's own Docker (`docker.io` and its Compose), from the distribution's own signed
  archive, adding no repository or key. When `download.docker.com` can't be reached (no network,
  a proxy that refuses it, a timeout or a server error), the install stops and says it couldn't
  reach it; nothing is installed. The repository checks of Terraform, Vagrant, the Vagrant VMware
  Utility and VirtualBox work the same way, stopping with a link to the vendor's page on a 404.
  On any other distribution, or one without apt, it installs nothing and links to Docker's own
  instructions.
- The AWS CLI goes to `/usr/local/aws-cli`, after its signature is checked.

**macOS.** Tools come from Homebrew, which must be installed first; casks open their vendor's
installer, which asks for an administrator password where it needs one.

## Privacy and what is sent

Once the user signs in, the launcher registers this machine with Cyber CTF (`cyberbackend.com`) as
one of the user's launchers, so the website can show it and start labs on it. It sends:

| Field          | Value                                                                                                                              |
| -------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| Install ID     | a random 32-hex-digit identifier made at first use (`install-id` in the app data folder)                                           |
| Machine name   | `<OS user name>'s Mac`, `'s PC` or `'s Linux`, from `USER`/`USERNAME`; or `CYBERCTF_AGENT_NAME` when set                           |
| OS             | the OS family and version, e.g. `macos 15.1`, `windows 11`                                                                         |
| Architecture   | `x86_64` or `aarch64`                                                                                                              |
| App version    | e.g. `1.8.9`                                                                                                                       |
| Capabilities   | `docker`                                                                                                                           |
| Launch targets | for each server and cloud account the user saved: its id, the name the user gave it, and its provider (no address, no credentials) |

It sends a heartbeat with the version, OS and launch targets about every 30 seconds and asks for
website-started launches about every 6 seconds while the app runs. For a website-started launch it
reports the launch's progress and, when it runs, the lab's local address. Starting a lab sends the
lab's id and where it runs. The launcher has no analytics or crash reporting; its logs stay on the
machine. To keep user names out of the machine name, set `CYBERCTF_AGENT_NAME` for the user.

A launch started from the website never runs unattended: the launcher brings its window forward
and asks the user to approve it, naming the lab, where it would run (this computer, or the server
or cloud account by name) and whether it has privileged machines. A decline, or no answer within
three minutes, refuses it and tells the website why. The [policy reference](reference/policy.md) says how an administrator
controls this.

Cyber CTF's privacy policy: <https://www.cyberctf.org/privacy>.

## Logs and diagnostics

The log folder (table above) holds `cyberctf.log` and one `deploy-<lab>.log` per lab, rotated at
2 MB with four files kept. What a lab operation's tools printed, and a deploy worker's crash
message, is in `deploys/<lab>.log` in the data folder, the previous operation's in
`deploys/<lab>.previous.log`. The level is `info`; set `CYBERCTF_LOG` (`error`, `warn`, `info`,
`debug` or `trace`) in the app's environment for another. Tokens, passwords, credentials in URLs
and values under secret-sounding names are redacted from every line. **Settings > About** has **Diagnostics > Copy** (the
version, the OS and the machine's setup status, for a support request) and **Log files > Open
folder**. Logs name labs, servers and paths: read them before sending them outside.

## Uninstall

First, in the launcher, **stop every lab** (_Stop & remove_ on the lab's page), above all those on
a server or in a cloud account: their Terraform state in the app data folder is what destroys
them. Settings > Maintenance > Cleanup removes leftover labs, the images and boxes the launcher
downloaded, and the tools it installed. A lab's VMs on this computer go with the lab; once the
app data folder is deleted the launcher can't find them any more, so one still running (in the
hypervisor's own app, named after the lab) is then removed there by hand.

The machine self-test removes its VM when it ends; one it left running (the test interrupted)
is named **CyberCTF test VM** and runs from a disk in the app's cache folder. The next test
removes it first. On uninstall:

- the NSIS uninstaller removes it from Hyper-V before it deletes anything, when the account may
  manage Hyper-V (an administrator, or a member of Hyper-V Administrators); only a VM of exactly
  that name whose every disk is in the cache folder;
- the MSI doesn't: it installs for the whole computer and leaves each user's data folders, so it
  never touches a VM running from one;
- anywhere else, remove it by hand before deleting the cache folder:
  `Stop-VM -Name "CyberCTF test VM" -TurnOff; Remove-VM -Name "CyberCTF test VM" -Force`
  (Hyper-V), `VBoxManage unregistervm "CyberCTF test VM" --delete` (VirtualBox), or in the
  hypervisor's app.

**Windows**

```powershell
# MSI
msiexec /x Cyber.CTF_<version>_x64_en-US.msi /qn
# NSIS (per user)
& "$env:LOCALAPPDATA\Cyber CTF\uninstall.exe" /S
# Store
Get-AppxPackage F4K.CyberCTF | Remove-AppxPackage
# Data, logs, web view data
Remove-Item -Recurse -Force "$env:APPDATA\org.cyberctf.desktop", "$env:LOCALAPPDATA\org.cyberctf.desktop"
# Credentials (repeat for each listed target)
cmdkey /list | Select-String org.cyberctf.desktop
cmdkey /delete:session.org.cyberctf.desktop
# Hyper-V Administrators, if the user was added
Remove-LocalGroupMember -SID S-1-5-32-578 -Member "$env:USERDOMAIN\$env:USERNAME"
```

**macOS**

```bash
brew uninstall --zap --cask cyberctf        # Homebrew; or delete /Applications/Cyber CTF.app
rm -rf ~/Library/Application\ Support/org.cyberctf.desktop ~/Library/Caches/org.cyberctf.desktop \
  ~/Library/Logs/org.cyberctf.desktop ~/Library/WebKit/org.cyberctf.desktop
while security delete-generic-password -s org.cyberctf.desktop >/dev/null 2>&1; do :; done
```

**Linux**

```bash
sudo apt-get remove cyber-ctf               # or delete the AppImage
sudo rm /etc/apt/sources.list.d/cyberctf.list /usr/share/keyrings/cyberctf.asc
rm -rf ~/.local/share/org.cyberctf.desktop ~/.cache/org.cyberctf.desktop \
  ~/.local/share/applications/cyberctf-desktop-handler.desktop
secret-tool clear service org.cyberctf.desktop
sudo gpasswd -d "$USER" docker              # if the launcher added the user to docker
```

On every platform `~/CyberCTF/shared` (the folder shared with the attack box) holds the user's own
files and is left in place.
