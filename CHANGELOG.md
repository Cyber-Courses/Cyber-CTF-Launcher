# Changelog

All notable changes to Cyber CTF Desktop are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **What a player does that breaks a lab, catalogued and tested.** [tests/resilience.md](tests/resilience.md)
  lists each disruption (a machine removed, stopped or powered off outside the app, the app or its
  deploy worker killed, Docker or the hypervisor stopped, files deleted, a double Start, Stop
  during Start, the network, the computer's sleep and clock), what the app must do and which test
  holds it. New integration scenarios (`resilience-*` in `tests/integration/run.sh`) do each
  disruption with the player's own tool on the real Docker or hypervisor; those that stop more
  than their own lab run only where `CYBERCTF_TEST_RESILIENCE_THROWAWAY=1`. New desktop specs:
  `stop-during-start`, `double-start`, `close-mid-deploy`, `kill-worker`.
- **The integration runner records a lab the launcher refuses on a hypervisor as skipped.** A VM
  scenario whose lab can't run on the hypervisor it is given (a VM on a network, on UTM) checks the
  launch's own refusal before it starts anything, and is recorded `skip` with the launcher's words,
  where it was a failure in a few seconds.
- **The attack box's credentials, on the lab page and its screen.** The box's `player` account
  now has a password, made by the launcher for each box it adds (20 random lowercase letters and
  digits without look-alikes, in four groups), so a prompt no longer leaves the player stuck: a
  desktop box's screen locker or login after a logout, a VM's console login, `su`, SSH. The lab
  page's attack box card has a **Credentials** row (`player` and the password, masked until
  **Show**, copied with a click), and the box's Screen window a **Sign-in** popover in its toolbar.
  The password is kept in the OS keychain (`attack-box:<lab>`), forgotten with the box, and
  redacted from the logs; the box gets only its SHA-512 crypt hash (RustCrypto's `sha-crypt` 0.6.0,
  MIT OR Apache-2.0), in a root-only file its set-up
  hands to `chpasswd` on standard input, never on a command line. `sudo` still asks for none. A box
  added before this has no password and shows no row.
- **Open in VirtualBox (or the lab's own hypervisor app).** Beside a running VM's Screen, on the lab
  page and the attack box, a button opens it in its hypervisor's own app where this computer has
  it: VirtualBox, Hyper-V's Virtual Machine Connection, virt-viewer (else remote-viewer), VMware
  Fusion or Workstation, Parallels Desktop, UTM, or the web console of ESXi or Proxmox VE in the
  browser; vagrant-qemu has no app, so none is offered. The runner says what opens it
  (`native_console`); the launcher runs the first way this computer has, without a shell. The
  built-in viewer stays the default.
- **Remove lab, on a lab's page.** A stopped lab's page has **On this computer > Remove lab**: an
  imported lab always, a catalogue lab once it has been started here. It asks first, naming the
  lab, then deletes the app's copy of it and everything the app keeps for it (the runner's run
  folder, an older launcher's emptied Terraform deployments, the deploy worker's job, logs and
  verdicts, leftover update copies, a catalogue lab's offline source, its attack box's password
  in the keychain) and goes back to the Labs list with a toast. The folder an imported lab came
  from is never touched, and a catalogue lab's progress stays in the account. It is disabled
  ("Stop the lab first") while anything of the lab is up, parked or left over, or an operation
  runs on it, and the core's new `lab_remove` refuses it then too. A file held open in the copy
  refuses the removal with nothing deleted; what can't be deleted after that (the password, when a
  locked keychain keeps it) is listed with why, the lab removed all the same.
- **A lab machine's screen, in its own window.** The lab page's Screens list each VM whose run
  offers a graphical console, with a Screen button (and why one has none: Parallels, VirtualBox
  without its extension pack, a cloud Linux VM), and the attack box's card has one. A VNC screen
  (libvirt, QEMU, VMware, Proxmox) is drawn in the window by noVNC 1.6.0 (vendored, MPL-2.0)
  through the window's own loopback WebSocket bridge: one viewer, a one-time token, the app's own
  `Origin` and the bridge's `Host` only, and that window's content security policy lets it reach
  that port alone. A Remote Desktop (VirtualBox's, a cloud Windows VM's through an SSH tunnel)
  opens in the system's client (Remote Desktop Connection, Windows App, FreeRDP, Remmina) on a
  `.rdp` file with no password; its password is copied on request. A new `screen-windows`
  capability grants those windows their three commands only.
- **The attack VM's desktop (Settings).** A VM attack box stays headless by default, or gets
  Kali's XFCE desktop with LightDM signing `player` in (about 1 GB more to download, 3 GB more on
  its disk, 4 GB of memory and 2 CPUs), which its Screen shows. A container box stays headless.
- **A lab page for a lab imported from a folder, built from what the folder defines.** An
  imported lab (`local-<name>`) is listed under **Imported from a folder** on the Labs screen, and
  its entry under **Running** in the sidebar, its row on the Overview and the command palette open
  its page, which none of them did. The page reads the lab's own `.ctf/` folder from the installed
  copy: `metadata.json` gives the title, description, difficulty, category and capabilities (else
  the title is the spec's name and the rest is left out, with no empty slots), and
  `objectives.json` the objectives, shown read only with a note that progress is tracked for
  published labs (no section without it). A file that can't be read is named on the page with
  why. Without `.ctf/`, the page is the lab itself: its state, deployment, network, where it runs,
  the attack box, and Stop, Open in browser and the shell. Nothing on it is asked of Cyber CTF,
  which doesn't know the lab, and it never asks to sign in; a stopped one starts again by
  importing its folder again.
- **Two ways to get VMware's OVF Tool for ESXi, and a third to use one already downloaded.** The
  ESXi setup's OVF Tool row links to VMware Fusion (macOS) or Workstation (x64 Windows and Linux),
  both free and shipping the tool, and to the standalone download. **Use a downloaded OVF Tool…**
  takes the folder the download unpacked to and copies it into the launcher's own `tools/ovftool/`
  folder, on its `PATH` without a restart; on macOS only once the program and its libraries are
  verified as signed by VMware (team `EG7KH642X6`), lifting the quarantine from that copy only.
  It says when the Intel-only macOS tool needs Rosetta. The row shows the version found and where
  (Fusion's or Workstation's copy first, then the launcher's, then `PATH`), and Workstation's
  bundled copy on Windows is now found.
- **An MSI installer for Windows x64**, for all users of a machine and for deployment with
  Intune, Configuration Manager or Group Policy: `msiexec /i Cyber.CTF_<version>_x64_en-US.msi /qn`.
  The NSIS installer (per user, `/S`) stays.
- **Each release says what it contains and where it comes from**: `SHA256SUMS` of every file, a
  build provenance attestation of each (`gh attestation verify <file> --repo
Cyber-Courses/Cyber-CTF-Launcher`), CycloneDX SBOMs of the Rust and npm dependencies, and
  `THIRD_PARTY_NOTICES`, also shipped inside the installed app and the Store package.
- **An administrator guide** (`docs/admin-guide.md`): silent install and removal, updates, the
  network endpoints and local ports, proxy and certificate behaviour, what works offline, the
  files, keychain items and machine changes, what is sent to Cyber CTF, and a clean uninstall per
  platform. The docs are reorganised by purpose (`docs/README.md`), with a getting-started
  tutorial, troubleshooting, a configuration reference, a versioning and support policy and the
  release process; the README is rewritten for users and IT, and the architecture moved to
  `docs/architecture.md`.
- **Governance files**: `SECURITY.md` (private vulnerability reporting, response times),
  `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md` (Contributor Covenant 2.1), `SUPPORT.md`, `CODEOWNERS`,
  issue forms and a pull request template.
- **A log level switch, and secrets kept out of the logs.** `CYBERCTF_LOG` (`error`, `warn`,
  `info`, `debug`, `trace`) sets the level of the log files for a run (`info` without it). Every
  log line, and every command a setup log shows, has its tokens, passwords in URLs, values under
  secret-sounding names, JWTs and AWS access key ids replaced by `[redacted]`.
- **A machine-wide policy for administrators.** An administrator can turn launcher features off
  for everyone on a computer: the website agent (`DisableWebsiteAgent`), the in-app updater
  (`DisableUpdater`), one-click tool installs (`DisableToolInstalls`), labs on cloud accounts
  (`DisableCloudLaunches`) and labs with privileged machines (`RefusePrivilegedLabs`). The launcher
  reads it at start from the OS's managed location: `HKLM\SOFTWARE\Policies\CyberCTF\Launcher`
  (`REG_DWORD`s) on Windows, the `org.cyberctf.desktop` managed preferences (an MDM profile) on
  macOS, `/etc/cyberctf/policy.json` on Linux. Settings, under About, lists what it turns off, and
  an operation it refuses says which setting did. A setting it doesn't know is logged and ignored;
  one whose value isn't a boolean, or a policy that can't be read, is logged and treated as on.
  `docs/reference/policy.md` has the reference.
- **The attack box on every form a lab runs in but ESXi.** The attack box is one more machine of
  the lab, wherever it runs: on Docker, in a VM on this machine, as VMs here, on a Proxmox
  server, or in a cloud account (containers on one VM, or one VM per machine); not yet on an ESXi
  host. Beside containers it is a container
  from the attack box image in Settings; among VMs it is a Kali VM with Kali's headless toolset
  (installed on its first start, about 16 minutes), named `attackbox`. On an Arm Mac it runs
  natively (Kali's arm64 image; a Debian VM made Kali). Its status, address and shell are the
  lab's own; starting or stopping it changes the running lab without restarting the rest. A lab
  that declares where the player sits (its own access machine) gets the box there, in place of
  that machine.
- **The launcher checks a cloud account's sign-in before it changes a lab there.** Before a lab
  in AWS, Azure, Google Cloud or Oracle Cloud starts, changes or stops, the launcher asks that
  cloud's CLI whether its sign-in still holds; when it ran out, it says how to sign in again
  (`aws login`, `az login`, …) and changes nothing, where Terraform used to fail halfway and leave
  part of the lab behind. A failure that says the same reads the same way.

- **Docker installs on Kali and the other Debian derivatives.** A distribution built on Ubuntu or
  Debian (Mint, LMDE, …) gets Docker's repository for the release it is built on. Where Docker's
  repository has no packages for the release (Kali's rolling one, Devuan's, a Debian or Ubuntu
  newer than Docker serves yet), the distribution's own Docker is installed instead: `docker.io`
  and its Compose, from the distribution's signed archive. The player joins the `docker` group
  either way.
- **Hyper-V can be turned on from the setup.** It used to be a link to Microsoft's guide. The
  setup turns the feature on through Windows' own servicing tool, after the UAC prompt, and asks
  for the restart that starts it; on Windows Home, which has no Hyper-V, it says so first.
- **A new AWS account starts on the AWS CLI's browser sign-in** (`aws login`). An AWS CLI older
  than 2.32, which doesn't have it, is named with where to update it; a profile already signed in
  and access keys still work.

### Removed

- **The attack VM box in Settings.** It chose the Vagrant box of a lab's own attack VM; the
  attack box among VMs is now always Kali, made by the launcher.

### Changed

- Bundles Isoloom 2.0.0 and Isothrum 2.0.0 from crates.io, which write and read the run
  manifest's version 1. Their changelogs list what changed.

- **A desktop attack VM never locks its screen.** Kali's screen lockers don't start for `player`,
  the screen never blanks, and the session doesn't lock on suspend or a switch of user, so the
  player isn't locked out mid-exercise; a login prompt that still shows takes the box's password.

- **A Windows lab with a flag or another input runs on no cloud.** Isoloom sets a Windows
  machine in the cloud up during Terraform's apply, before the runner delivers its inputs, so AWS
  and Azure (the clouds that took it) now show as refused in the launch picker with that reason,
  and its VMs run on this computer or on ESXi. The ways a lab runs, and its requirements, are
  Isoloom's own answer (`Lab::possible`), the one its generator gives.
- **The launcher draws its own title bar on Linux and Windows, built into the window as on
  macOS.** Every window (the main one, Settings, the setup windows, the attack box shells) opens
  without the system's frame. There is no separate bar: as on macOS, the sidebar runs to the
  window's top and the page's own header (the breadcrumb and engine status in the main window)
  is the title bar, with minimize, maximize/restore and close in its top-right corner, the
  height of the header, in the chosen appearance (Dark, Black or Light), the close button red
  under the pointer. They are reachable from the keyboard and labelled in the app's language.
  Under XFCE and other window managers that draw the frame from their own theme, a dark launcher
  no longer sits in a white frame. The header drags the window, a double-click on it maximizes
  or restores it, and the window still resizes from its edges, clear of the buttons; on Windows
  it keeps the system's shadow and thin border, on Linux a hairline draws its edge. Close goes
  through the same check as the system's button: with a lab deploying, the launcher still asks
  first. A crash screen has the header too, so a crashed window can still be moved and closed.
- **On a Linux without apt, an install says to install the tool by hand.** On Fedora, openSUSE
  or Arch the setup used to ask for the password, then fail with `apt-get: not found`; it now
  stops before anything runs.
- **libvirt's install brings what the vagrant-libvirt plugin builds against** (`libvirt-dev`, a
  compiler, `pkg-config`, `ruby-dev`), where the plugin's install failed building, and adds the
  player to the `libvirt` and `kvm` groups. Until the next login, which those groups need, the
  machine check shows libvirt as not ready and the setup asks to log out and back in.
- **Removing something asks in the system's own dialog.** Removing a cloud account or a server,
  Stop & remove, removing a tool or the downloads, and restarting Windows for WSL ask in a native
  dialog, and only go ahead on its yes.
- **The Cloud page lists only the connected accounts**, with each provider not connected offered
  apart as a Connect button with its logo. Removing an account or a server drops that one saved
  host, and refuses when more than one has its id.
- **The ESXi VM test runs a real VM.** It used to check only that the host's SSH port answered.
  It now runs a throwaway one-machine lab on the host the way a lab runs there: Vagrant's
  `vmware_esxi` through the Isothrum runner, with the host's settings and password. The steps
  are: reach the host; prepare the test lab; upload the test VM to the datastore with the OVF
  Tool, its box named with its architecture; start the VM; the VM gets an address on the host's
  network; reach it over SSH; run a command inside it; remove everything the test created. The
  teardown runs even after a failure and says what is left when it fails. A test cut short
  keeps its folder, and the next test removes its VM first. Before anything reaches the host,
  the test checks that this computer has the `vagrant-vmware-esxi` plugin and the OVF Tool, as
  the Machine page finds them, and says which one is missing.
- **`pnpm release-check` runs every check CI runs on a pull request but the Terraform templates'
  validation**: it gained the docs' links, the build, the generated bindings check, clippy for
  Linux and Windows (with `rustup`), `cargo deny`, shellcheck and actionlint (when installed), the
  actions' SHA pins, the Store listings' check and `pnpm audit --prod`. A runner it runs as a step
  (clippy for the other platforms, the desktop end-to-end) that skipped some of its own steps
  reads `skip`, not `ok`.
- **Licensed under the Apache License 2.0** (`LICENSE`, `NOTICE`).
- **A tag build refuses to ship unsigned installers.** The release stops before building when a
  Windows (Azure Artifact Signing), macOS (Developer ID, notarization) or updater signing secret is
  missing, where Windows installers used to ship unsigned, and it verifies every signature it made
  (`Get-AuthenticodeSignature`, `codesign --verify --deep --strict`, `spctl`). The signing tool is
  pinned to a version.
- **The Homebrew cask is kept in this repository** (`packaging/homebrew/cyberctf.rb`) and its
  `zap` also removes the logs and the web view data; it says what zap leaves (the keychain items,
  the shared folder).
- **Dependencies are checked and kept current**: `cargo deny` (advisories, Apache-2.0-compatible
  licences, crates.io only) and `pnpm audit --prod` in CI, Dependabot for the crates, the npm
  packages and the pinned actions, and a link check of the docs (`pnpm check:docs`).

- **The accessibility check fails on serious issues too.** The axe check in the web end-to-end
  tests failed only on critical issues and contrast; it now fails on serious ones as well, and
  covers the launch prompt and Settings as an administrator manages it. The one serious issue it
  found, the command palette's results list scrolling out of keyboard reach, is fixed: the list is
  a tab stop of its own.
- **Docker Desktop's licence is said where the app offers it.** The one-click Docker install on
  macOS and Windows, and Docker Desktop's card, say it needs a paid subscription in larger
  companies, and name the engines Cyber CTF also runs labs on: Colima and Podman (free) and
  OrbStack (free for personal use).
- **The webview loads remote images from one host.** Its content security policy allowed images
  from any `https:` address; it now allows the app's own, `data:` and
  `https://avatars.githubusercontent.com` (the account picture of a GitHub sign-in). Any other
  picture shows the account's initials.
- **Corporate proxies reach the tools the app starts.** On macOS, an app opened from the Finder
  doesn't get the shell's proxy: when no proxy variable is set, the system proxy (MDM-set or from
  System Settings, as `scutil --proxy` reports it) is passed to docker, vagrant, terraform, git
  and the cloud CLIs as `HTTP(S)_PROXY`/`NO_PROXY`, with this machine's own addresses bypassed (a
  PAC file can't be, and the log says so). On Linux, one-click installs run through
  pkexec, which clears the environment: the proxy variables are now handed to them. The ESXi relay
  on macOS keeps a run's `no_proxy` entries instead of replacing them. A proxy variable set to
  nothing counts as unset, so the system proxy still applies.
- **No vendor install script runs as root on Linux.** The one-click installs of Azure CLI and
  gcloud set up Microsoft's and Google's apt repositories, and VirtualBox's Oracle repository, each
  with its signing key checked against a fingerprint pinned in the app, as Docker's and
  HashiCorp's already were; Docker on a distro outside the Debian family says where to get it
  instead of running get.docker.com. gcloud is now a system package (`google-cloud-cli`)
  rather than a per-user SDK in `~/google-cloud-sdk`; removing it from Cleanup removes both.
  Every vendor's repository is set up the same way: its key in `/etc/apt/keyrings/<name>.gpg`
  and its source, for this machine's architecture, in `/etc/apt/sources.list.d/<name>.list`.
- **Every lab launched from the website asks on this computer first.** Only a launch onto a cloud
  account used to ask; one here or on a server ran unattended. Now each one shows the lab's name,
  where it would run (this computer, a server or a cloud account, by name) and, when the lab has
  privileged machines, that they get kernel-level access; it runs only once approved here, and a
  decline or no answer within three minutes refuses it. A launch the policy refuses is refused
  before it is asked about.
- **The in-app updater stays out of a package manager's way.** A copy installed by a Homebrew
  cask, a Debian package or the Microsoft Store is updated there, so the app no longer checks for
  or offers its own update (the updater plugin isn't started); Settings says where its updates
  come from. The `DisableUpdater` policy turns it off the same way.
- **Tests, scripts and docs, reorganised.** End-to-end tests are under `e2e/` (`web/` with
  Playwright, `desktop/` with WebDriver), the integration tests (real labs on real tools) in
  `src-tauri/src/integration_tests/`, run by tier with `pnpm test:integration`; the Proxmox and
  ESXi builders moved to `infra/test-servers/` and the Windows installer smoke to `packaging/`.
  The suite runners share `scripts/lib/suite.sh` and take `--help`; `docs/` has an index, how labs
  run (`running-labs.md`) and the testing guide (`docs/testing/`).
- **"Add a provider" on the Machine page is a provider picker.** It opened the lab-type guide,
  which on a set-up machine had nothing to add (both paths "ready", Review alone). It now opens
  "Add a provider": the container engines and hypervisors this machine can run but doesn't
  have, each with its logo, a note and its install (the one-click install where Cyber CTF has
  one, else where to get it and a re-check), its install log, and, once the machine check finds
  it, its own test right there and "Use by default" once the test passes; a hypervisor also asks
  for Vagrant and its plugin first. With nothing left to install it says so. The lab-type guide
  stays for the first run.
- **The lab page shows where a lab runs, with its logo.** The network diagram's host card
  showed "Your machine · 127.0.0.1 · host" and a laptop whatever the lab ran on. It now carries
  the logo and name of what runs the lab: the container engine for containers here (the one
  the lab was started on, recorded at launch, else the one the Docker CLI points at), the
  hypervisor for VMs here, the server and its kind (Proxmox VE, VMware ESXi) with its address,
  or the cloud with its region and the account's name. The header's "on …" and Details' "Runs
  on" say the same thing: "on this machine · OrbStack", "on this machine · QEMU", "on homelab ·
  Proxmox VE", "on AWS · eu-west-3". A container lab's status now names the engine it runs on
  instead of a bare "docker".
- **The Machine page lists its providers.** The Health/Setup switch and the setup
  checklist are gone: the page shows this machine's health and every container engine and
  hypervisor found on it as a row, each with its own state (the machine check now asks every
  Docker CLI context in turn with `docker --context <c> info`, so Docker Desktop, OrbStack,
  Colima, Docker Engine or Podman side by side each read running or stopped, not only the one
  the CLI points at; the current engine and the hypervisor labs run on are marked). Every
  running engine and every ready hypervisor has its own Test: the container test runs on that
  engine's Docker context, the VM test on that hypervisor, and each row keeps its own last
  result ("tested 5 min ago", or failed and why), the lab type's own readiness following only
  the engine or hypervisor labs run on. "Use by default" on a row points Docker at that engine
  or makes that hypervisor the Settings choice. Only what is installed is listed, in the
  Providers panel and in Details alike: nothing for the engines and hypervisors this machine
  lacks; adding one is what "Add a provider", in the header as the Servers page's "Add server",
  is for (see above). A row's Fix still opens its setup step directly.

### Fixed

- **A lab VM deleted in Proxmox's web UI shows gone.** A Proxmox lab's page read its run's own
  Terraform state, which still held the VM: it read as running until the next stop. The status
  now asks the server's API where each VM stands, signed in with the server's saved token or
  password: a VM removed there shows gone, as a container removed outside the app does (the health
  banner, Reset), and one powered off there shows off. An ESXi lab's status already asked its host.

- **A server that refuses its saved credentials says so, with the way to fix it.** A Proxmox API
  token revoked, or an ESXi host's password changed, showed only as the step's raw failure. The
  runner now reports the server's refusal (`server_auth_failed`), and the lab page says "Cyber CTF
  can't sign in to <server>: its token was revoked or its password changed. Update it in Servers."
  (the server by its name in Servers), with **Open server settings**, on a failed start, stop or
  park and on a status that can't be read, in every language.

- **A container stopped outside the app can be started again from the lab page.** A Docker lab
  with a container stopped by `docker stop` or Docker Desktop's Stop was offered Reset only, though
  a resume brings it back as it is (Compose starts its stopped services). The health banner now
  offers **Start them** on a Docker lab too, while every machine that went down is still there;
  a container removed outside the app can't be resumed, so the banner offers Reset alone then, on
  a VM lab too.

- **A VM box download that stalls or fails says so as the box.** The boxes a Vagrant start boots
  are downloaded in a step of their own (`boxes-<step>`); one that stopped making progress or
  failed was told as "the lab's images". It now reads "Downloading the lab's VM box stopped making
  progress (nothing new for 15 minutes) …" or "Couldn't download the lab's VM box: <Vagrant's
  reason> …", with the network hint, in every language.

- **A clock jump no longer makes a starting lab's worker read as lost.** A deploy worker was
  recognised by its start time, which Linux computes from the wall clock: once the clock jumped
  (NTP, set by hand, a VM restored), the live worker read as another process, its pidfile was
  swept and its start settled as lost while it ran on. It is now recognised by its boot and its
  start within it, as the kernel stored them (Linux: `/proc/<pid>/stat` and the boot id; macOS:
  the start stored at the fork and `kern.bootsessionuuid`; Windows: the creation time and the boot
  number). Pidfiles written before keep the start-time match.

- **A lab whose deploy worker was killed reads as failed, and its launch token is removed.** A
  worker killed mid-start (Activity Monitor, Task Manager, a crash, the computer switched off)
  wrote no verdict: the lab read as running on whatever its start had brought up, set up or not,
  and its job file, with the launch token in it, stayed on disk. The app now writes the verdict a
  dead worker left out when it finds it (following it, listing the running workers, or reading
  the lab's status): "The deploy stopped before it finished…", the lab's last launch failed, the
  job file removed, and the failure says to stop the lab to clear what it left. Isothrum's runner
  now also takes a run whose operation's process died as failed, so its next start clears what
  it left instead of saying the lab already runs.
- **A machine removed outside the app is shown gone.** A container removed with `docker rm -f`
  (or a VM deleted in its hypervisor) while its lab ran disappeared from the lab page, which drew
  a healthy lab without it and offered no way back; it is now shown `gone`, with the page's health
  banner and its reset. A resume no longer brings the lab back without it (Isothrum): it says
  which machines are gone, and a start again builds the lab afresh.
- **Docker quit while a lab starts says so.** A container lab whose Docker stopped during its
  start said "`lab step up` failed: Cannot connect to the Docker daemon…"; it now says Docker isn't
  running and to start it, as the start's own check does.
- **A running lab whose generated files were deleted can still be stopped.** Deleting the lab's
  `.isoloom-<n>/` folder while it ran made its page read it as stopped, and its Stop said "Nothing
  to remove" and left its containers or VMs running. The files are made again from the lab's
  spec; with the lab's whole folder gone, the Stop says what may still run, by name.
- **"Sign in to start" on a lab's page goes on into the start again.** Once signed in, the
  catalogue is read again for the account, and while it loaded the lab's page was swapped for a
  loading one and back, which dropped the start the player had asked for: they were left in front
  of the Start button they had already pressed. The open lab's page now stays while the catalogue
  reloads, and "Where should it run?" opens as soon as the sign-in goes through.
- **A slow download of a container lab's images no longer reads as Docker running out of
  resources.** Starting a lab with the attack box (an 11 GB image) on a slow connection was stopped
  with "The lab's `up` step didn't finish within 30 minutes … check the resources of Docker, the
  hypervisor or the server", and every retry hit the same wall. The runner now pulls a container
  lab's images before starting its containers, with no time limit while the download moves; the
  lab page shows that download under **Get images**. A download that stops moving for 15 minutes
  says "Downloading the lab's images stopped making progress", and one that fails says "Couldn't
  download the lab's images:" with Docker's reason, in the app's six languages, with the network
  hint. A step that runs out of time still says to check the network and the resources.
- **A lab's status no longer turns red while another process is adding a VM.** With the attack
  box's start provisioning `attackbox` in the deploy worker, the page's status poll failed with
  Vagrant's lock refusal, `MachineActionLockedError` ("An action 'read_state' was attempted on
  the machine 'attackbox', …"). The runner now reports that machine as busy, the others as last
  read: the page says it is being set up (not down), offers no screen for it meanwhile, and reads
  it again on the next poll.
- **The Screen button is offered only for a screen that is there now.** A VM's screen is offered
  once it runs and the runner found its console answering; otherwise the page says why (a
  vagrant-qemu VM started before screens were given one says to restart it). A screen asked for
  while the lab was being changed no longer waits on Vagrant's locks.
- **The screen window never spins for good.** The bridge gives the VM's screen 10 s to accept and
  15 s to speak, and the viewer 30 s to show it; past that, the window says where the screen
  didn't answer (`The VM's screen didn't answer at 127.0.0.1:5903.`) with Try again.
- **Why a hypervisor can't run labs is said in the app's language.** The Machine page, the
  setup's VM step and Settings showed the core's English reason in every language; they now say
  it in the six. **Allow my account** is offered only when the account lacks the right to manage
  Hyper-V, not for any Hyper-V that isn't ready (Vagrant missing, say).
- **A lab's Stop in the list offers the lab page's Shut down for a VM lab run here.** The row
  judged whether a lab can be shut down by where it runs, never in a VM here; it now asks the same
  controls the lab page does.
- **A lab check that couldn't run says so.** When a machine of a running lab went down, a check
  that failed to run (the runner busy, the lab out of reach) read as the lab no longer solvable.
- **A launch with nowhere to run is refused before anything is downloaded**, saying why: a
  cloud lab with no account picked, a VM lab with no hypervisor here (both in the app's
  language), one VM per machine with no hypervisor here or on a server rather than a cloud
  account, ESXi or Proxmox picked with no host, and a start on a cloud account the policy turns
  off (`DisableCloudLaunches`).
- **A failure read back after a reload gets its hint.** A deploy that ran through a window reload
  or a relaunch is known by its error's code, so its hint (a port taken, Docker not running, …)
  shows whatever the error's wording.
- **A lab page says why its objectives couldn't be read**, instead of showing only the lab's
  question as for an older backend.
- **Settings names the installed tools as the setup installed them** (Docker Engine on Linux,
  Docker Desktop elsewhere), and an operation the app ran itself shows in the sidebar as what it is
  (a shutdown, a resume or a provisioning run, not a pause or a launch).
- **An empty machine name is refused** before a provisioning run starts.

- **The attack box's shell opens as the player's own account, `player`, on every form.** It
  opened as whatever account the way in logs in as: root in a container, `vagrant` in a VM (on
  Windows' Hyper-V as elsewhere), a cloud image's user on a server or in a cloud account. The box
  now has `player` everywhere, with a home and sudo without a password: the attack box image is
  built with it, a container box makes it where it starts (its image may predate it) and a VM box
  at the end of its set-up; the shell opens as it through Isothrum's console as a named account
  (`exec --user` into a container, a `sudo -i` login on a VM). Where the shared folder belongs to
  a user the box doesn't have (a Linux computer's), `player` takes that user's uid, and joins the
  folder's group, so it writes there. Root stays the image's own user, and the lab's other
  machines open as before.

- **VMware Fusion and Workstation no longer read "Needs setup" again after the setup finishes.**
  Vagrant drives them through two parts, the `vagrant-vmware-desktop` plugin and HashiCorp's
  Vagrant VMware Utility (a system service), but the setup only linked to the utility and the
  machine check never looked for it, while the steps, the Machine page and the VM test each judged
  "set up" by a hypervisor of their own choosing. The machine check now lists what each hypervisor
  still needs (Vagrant, its plugin, the utility installed and its service answering on
  `127.0.0.1:9922`), it is ready only when that list is empty, and the setup steps, their "done"
  and the Machine page read that list. The setup installs the utility from HashiCorp's signed
  package, pinned with its SHA-256: on macOS checked again as root, as signed by HashiCorp's
  Developer ID (team `D38WU7D763`) and notarized, then installed with macOS's password prompt; on
  Windows checked again elevated, with its Authenticode signature, then installed with `msiexec`
  through UAC; on Linux from HashiCorp's apt repository, its key pinned. A failed install says so on
  its row, a row's **Fix** sets up that hypervisor and tests it, and a machine check made while an
  install ran no longer stands for minutes after it.
- **Every window follows the appearance from the moment it opens.** The attack box shell and the
  setup windows set no theme: on Windows their title bar opened white over the dark app, and
  they kept the colours they opened with when the appearance changed in Settings. Each window now
  opens in the main window's theme and follows a change made in another window, its page and the
  theme of what the system still draws for it (the taskbar's, its dialogs').
- **A VM lab's page no longer says it can't read the lab while the lab's own start, attack box or
  set-up is under way.** Vagrant refuses to read a VM's state while it boots and provisions it,
  for minutes, and the page's status poll, after a minute of retries, showed a red "Couldn't read
  the state of" the lab with Vagrant's `MachineActionLockedError`, though the start carried on
  (VirtualBox, QEMU, Hyper-V alike). With the runner reporting such a run as starting (or as last
  read, while the attack box is added) without asking Vagrant, the read succeeds; and a read that
  fails while one of the lab's own operations runs is no longer shown as a failure, the
  operation's progress being what the page shows then.
- **A long step of a server's VM test no longer looks frozen.** The ESXi test's upload of its
  VM sat for many minutes on one line with no sign of life: the OVF Tool redraws its progress
  (`Disk progress: 8%`) in place, which the runner never passed on as a line. With the runner
  reading those redraws, the upload step now shows **Uploading: 8%** as it moves (and so does a
  lab's machine step on the lab page while its VM uploads to ESXi). Every step of a server's VM
  test shows how long it has been running, then how long it took, and the upload step says up
  front that each test uploads the VM's image, with its size once the box is on this computer
  (about 2.4 GB for Debian 12), which can take several minutes.
- **A lab an older version left on its own Terraform is no longer said to be removable.** When the
  Isothrum runner couldn't take over such a cloud or Proxmox deployment, the notice said Stop and
  the auto-stop would still remove it, but nothing did and a cloud lab kept billing. The notice
  now says so and to delete it in the cloud console or on the Proxmox server, and a Stop that
  leaves it up says the same instead of reporting the lab stopped. A deployment whose run record
  can't be read is no longer handed over without its auto-stop time; that is said too.
- **A lab run on Rancher Desktop keeps its engine.** The lab page took it for the current engine
  and named it "Docker"; the engines and their names now come from the core.
- **A Stop fails when the lab's server marker can't be removed**, instead of reporting the lab
  stopped while its status, stop and the server's removal still went to that server.
- **An updated lab no longer inherits where its old copy ran**, which could send its status and
  Stop to a server it never ran on.
- **Azure's Test says when it couldn't ask Azure** (signed out, throttled, no network) instead of
  saying the lab size isn't offered in the region.
- **A launch on a chosen container engine says when Docker's contexts can't be listed**, not that
  the engine has none.
- **The Cloud page says why a Stop failed or was refused**, also when another operation on the lab
  was running.
- **The Downloads panel says when what Cyber CTF downloaded can't be read**, instead of showing
  "No lab downloads yet". **The Azure account setup says why its subscriptions couldn't be read.**
- **Try again on a failed machine check keeps spinning until the check answers**, and can't be
  pressed again meanwhile.
- **A Mac is offered the hypervisor made for its CPU by the core**: VirtualBox is no longer
  offered for install on Apple Silicon, nor UTM on an Intel Mac (one already installed stays).
- **A Proxmox user with a `!` but no realm is no longer taken for an API token**; a token is
  `user@realm!name`, as the core reads it.
- **A lab page's memory needs are in the decimal GB the rest of the app uses** (1024 MB now reads
  1.1 GB).
- **A lab's objectives no longer show another lab's progress** when a re-read answers after
  switching labs.
- **The Cloud page finds the labs running on an account by the account itself**, not its name, so
  a server with the same name no longer counts.
- **An auto-stop that finds no account recorded for a lab says so in the player's language.**
- **A VM lab on Hyper-V no longer fails to start while the app is open.** The sidebar's and the
  Machine page's count of running VMs asked `vagrant global-status --prune`, which reads every
  VM's state live; on Hyper-V that holds each VM's Vagrant lock for a moment, so a lab's own
  Vagrant command landing then was refused ("another process is already executing an action on
  the machine") and the start failed. The count now reads Vagrant's index as it is, and skips a
  record whose lab folder is gone.
- **A Proxmox server's key setup no longer says to run `echo '' >> ~/.ssh/authorized_keys`.**
  When the launcher's public key couldn't be read, the instruction to authorize it on the node
  added an empty line; it now says the key can't be read and to set the server up again.
- **A server's or cloud account's secret that the keychain can't read says what to do.** On
  Linux the sign-in already said to start and unlock a keyring; a server's password or an
  account's keys said only "keychain: …". Both now give the same advice.
- **Stop on the Machine page says why when it fails.** A lab or test that couldn't be stopped
  stayed in the list with no word; the failure is now shown.
- **A Google Cloud account that can't launch no longer passes Test or reads as signed in.** The
  setup and Test checked gcloud's own sign-in, while every launch first checks the
  application-default sign-in Terraform uses: when that one ran out alone, Test passed and the
  setup said "signed in as …", but each launch was refused with "sign in again". Test's first
  check and the setup now ask what a launch asks, on AWS too.
- **A damaged record of the tools Cyber CTF installed is no longer erased.** A record that
  couldn't be read counted as no tools, and the next install or removal saved that over it, so
  Cleanup stopped offering every tool recorded before. It is now left as it is and logged, and a
  removal says it can't read it. It is written as Storage's record is, under a file lock and
  synced to disk.
- **A session that can't be read no longer reads as signed out.** When the keychain couldn't be
  read (locked, say) or a token couldn't be renewed, the catalogue loaded as anonymous, without
  the player's progress, and the website agent forgot this machine's registration, with nothing
  in the log. The catalogue now says why it couldn't load, the agent keeps its registration and
  logs that it waits, and a piece of a long session the keychain couldn't read is that error, not
  a missing session.
- **A lab operation never reports the last run's result as its own.** When the last run's
  result file couldn't be removed (a permissions problem, a file held open on Windows), the new
  run's worker started anyway and the app read the old result at once. The operation now runs
  inside the app instead, and a job file (which holds the lab's launch token) that can't be removed
  is logged.
- **A lab whose last start failed no longer reads as running when its verdict is a plain
  message.** A deploy worker's verdict was read two ways: following the start counted a plain
  message (an older version's form) as a failure, but the lab's status counted it as success, so
  the lab showed as running. Both read it the same way now.
- **A lab whose start failed before its download finished no longer also reports "The attack box
  couldn't be added with the lab".** With the attack box set to start with each lab, the end of
  such a start read the lab's status, which refused a lab that isn't installed. A lab that isn't
  installed now reads as not running, and nothing is added.
- **A deploy worker that stops without a result says how.** The error names how its process
  ended (its exit code, and what a crash's code means, or the signal that killed it) and the last
  line it wrote, and the app's log says where its output is. A panic in a lab operation is now its
  result ("the deploy worker crashed: …") instead of leaving the lab starting for good; a worker
  that can't read its job, or can't start, says so as its result. Starting a lab again keeps the
  failed run's output in `deploys/<lab>.previous.log` instead of emptying it.
- **Signing in as another account mid-launch no longer reports the first one's machine
  online.** While a website launch ran, the heartbeat kept sending the agent registered for the
  account signed in before, with the new account's sign-in. It now waits until this machine is
  registered for the account signed in.
- **A relay to an ESXi host that can't start says so in the log.** On macOS, the loopback relays
  Vagrant reaches an ESXi host through (SSH, HTTPS, the WinRM proxy for Windows guests, the
  ovftool wrapper) failed silently when one couldn't start, as did a relay that couldn't reach
  the host; each is now logged.
- **Adding the attack box to a lab in a UTM VM no longer leaves the lab down.** Adding a machine
  restarts the lab's VM for its new settings; when UTM refused the boot with its passing
  "Application isn't running (-600)" after the VM had shut down, the update was tried again and
  refused ("This lab isn't running"), so the box was never added and the lab stayed stopped. When
  the refused attempt left the lab down, the launcher now starts it again on its current files,
  which brings the new machine with it; otherwise it tries the same operation once more, as
  before.
- **A lab's "ready" notification names where it runs.** A lab launched from the website on a
  server or a cloud account was announced as "now running on this machine"; it now names the
  server or account. A container lab started from the app on a cloud account said "running on
  your server"; it now says "in your cloud account".
- **A launch on a cloud account the launcher can't read says its budget wasn't checked.** An
  account missing from the saved hosts, or a `server.json` that couldn't be read, passed the
  monthly budget check as under budget, without a word; the launch log now says the budget wasn't
  checked and why.
- **A server's secret that the keychain won't give is no longer read as never entered.** A
  locked keychain, or its unlock prompt dismissed, made a saved host ask for its password again
  ("enter the host's password") and an AWS account's budget check say its keys were never stored.
  Both now say the keychain couldn't be read, and why. While the keychain waits on its prompt, the
  rest of the app keeps running. In a development build, a secrets file that can't be read is an
  error, where saving one host's secret used to erase every other's; it is written whole and
  owner-only from the start.
- **The attack box beside a container lab run as VMs failed on a missing Vagrant box.** On a lab
  deployed "one VM per machine" on this machine, "Start attack box" sent the attack box image
  (`cyberctf/attack-box@sha256:…`) where the runner expected a Vagrant box, and Vagrant Cloud
  answered 404. Among VMs the attack box is now the Kali VM the launcher makes (see Added), and
  the panel calls it an attack VM.
- **Labs made by an earlier version keep working after an update.** A lab's generated files hold
  a run manifest of the version that wrote them, and this version reads manifest version 1 only,
  so a lab left running or paused across the update couldn't be stopped, paused, checked or
  shown, and its old Terraform deployment couldn't be handed to the runner. Its files are now
  generated again for where it runs before any of those, with its instance and the launcher's
  own settings. A lab whose files can't be generated any more (its `isoloom.yml` gone) says to
  remove it and add it again, and that what it still runs is on the Machine page.
- **Sign out says when the keychain kept the session.** A keychain that refused to delete it
  (locked, its unlock prompt dismissed) went unnoticed: the app showed signed out, and the next
  start signed the player in again. Sign out now says it couldn't, and the app stays signed in.
- **An answer to a website launch that came too late says it started nothing.** Approving just
  as the three minutes ran out closed the prompt as if the lab were starting; nothing started and
  nothing said so. The prompt now says the request expired, and it counts down from when the
  launcher stops waiting rather than from when it was shown.
- **The Proxmox server test follows its steps when a credential is named like the provider.**
  The run hides a saved secret's value in every line it logs; a token or password containing
  "proxmox" hid the start of the lines that end each step, and the test showed its first step
  until it finished. Steps are now read from what can't be hidden.
- **Settings shows whether the website really sees this machine.** Every signed-in machine read
  "Online", even with the website agent turned off by the `DisableWebsiteAgent` policy or before
  it had registered. Settings now shows it online, registering, or turned off by the
  organization, and follows it as it changes; the sidebar's online dot on the account follows it
  too.
- **Settings says when it couldn't read something.** A machine check, version or agent read that
  failed looked like one still running ("Checking hypervisors…", "Registering…") for good. It
  now says what failed and why, with a way to check the machine again.
- **A failed quit says so.** "Quit anyway" or "Finish in background" that failed only enabled
  its buttons again; it now says it couldn't quit, and why.
- **The VM test removes a test VM an earlier run left behind.** A test VM left running that
  Vagrant no longer knew (its state lost, as with the app's data at an uninstall) held its disk,
  and every later VM test failed at `vagrant up` ("the file is being used by another process" on
  Hyper-V) until someone removed it by hand. The test now finds it by its name and removes it
  first, on Hyper-V when all its disks are in the test's folder and on VirtualBox when its
  description names that folder; a VM of the same name that is anyone else's is left alone, and
  the test says so. The NSIS uninstaller removes a Hyper-V test VM the same way before it deletes
  the app's folders.
- **Setting the machine up checks it far less often.** The guided setup and the onboarding
  checked the machine every 5 seconds, each check starting `vagrant` twice (a second or two of CPU
  each on Windows) and the WSL probe, and logging a WSL line every time. They now check when they
  open, when their window comes back to the front, after an install, and otherwise once a minute
  (every 5 seconds only while a check is failing). Vagrant's version and plugins are kept 10
  minutes (forgotten when the app installs or removes a tool), the running-VMs scan 30 seconds,
  and the WSL check is logged when its result changes.
- **What the policy turns off isn't offered.** The launch picker offered a cloud account under
  `DisableCloudLaunches`, and every place for a privileged lab under `RefusePrivilegedLabs`, and the
  tool install and remove buttons stayed on under `DisableToolInstalls`; each refused only once
  clicked. The picker now shows those places as refused, with the setting that refuses them (a VM
  lab's "VM" row on this machine too), and the buttons are disabled with a line saying why. Resuming a paused or shut-down lab with a
  privileged machine is refused under `RefusePrivilegedLabs` too, where it used to boot it again.
- **Browsing the launch picker leaves Settings' hypervisor alone.** Picking a hypervisor for a
  container lab's VM in the "Where should the lab run?" dialog, even one then cancelled, made it
  the Settings choice, and the next launch skipped the question. Only "Don't ask again" saves it
  now, with the rest of the path.
- **A UTM start is tried once more when UTM is too busy to answer.** Vagrant's UTM plugin reads
  a VM's forwarded ports through Apple events, and a busy UTM let one time out ("AppleEvent timed
  out (-1712)"), failing the start. That is now taken as passing, like "Application isn't running
  (-600)": the log says so and the start (or the update of a running lab) is tried once more.
- **Removing a server or account says when its secret stays in the keychain.** A keychain that
  refused to delete the host's password or token went unnoticed: the host was removed and its
  secret stayed behind. The secret now goes first, and when it can't, the host stays and the
  page says why.
- **Stopping containers from the Machine page says when Docker couldn't be asked.** A stop of a
  lab's leftover containers that Docker refused to list (its socket denied to this account) read
  as done while they kept running. It now fails with Docker's answer.
- **Signing in or out no longer shows the other state's labs for a moment.** The catalogue and
  the completed labs of before the change stayed on the Labs and Home screens until the new
  catalogue answered; the screens now wait for it.
- **A tool a stop cuts short never outlives the launcher.** The lab engine now asks a tool it
  cuts short to stop (Terraform then saves its state) and gives it time; a deploy worker being
  stopped waits for it, up to 15 seconds, and ends what is left before it exits, and the app
  ends any still stopping when it quits.
- **A cloud-services lab gets the account's region, subscription or project.** The lab's own
  Terraform module was never given where the account works when its provider names none: the AWS
  region (`AWS_REGION`), the Azure subscription, the Google Cloud project and region. They are now
  given from the picked account, unless the lab's `cloud.env` sets them, and a lab brought over
  from an earlier version keeps the region it was applied in.
- **A lab whose state can't be read says why.** When reading a lab's state failed (Docker not
  running, its run record unreadable, its files no longer able to be generated again), the lab
  page went on showing the last state it had read, with no word of it. It now says, at the top,
  that the state couldn't be read and why, until a read succeeds.
- **An update waits for a lab that is stopping or pausing, not only one that is starting.** The
  update install quits every Cyber CTF process, but it waited only for starts, resumes and
  provisioning runs: a Stop (which runs in the app) could be cut off mid-teardown, leaving a cloud
  lab's resources billing. The update and the quit prompt now ask the same rule, which counts
  every lab operation in flight.
- **A Docker install that can't reach Docker's server says so instead of installing the
  distribution's Docker.** On Ubuntu, Debian and their derivatives, any failure of the check of
  Docker's repository (no network, a proxy, a timeout) read as "Docker's apt repository has no
  packages" for the release, and the distribution's older `docker.io` went in. Only an answer that
  the release isn't served does that now; anything else stops, saying download.docker.com couldn't
  be reached.
- **A read that failed is said as one.** When a cloud lab's estimate couldn't be read, the launch
  dialog said "This lab gives no cost estimate" on the click that starts billing; a brief that
  couldn't be read read as "No briefing for this lab yet"; and a Machine page whose list of what
  runs couldn't be read said "Nothing running", dropping the list it had shown. Each now says it
  couldn't read it, and why; the Machine page keeps the list last read.
- **On Linux, Vagrant comes from HashiCorp's repository.** The guided setup installed the
  distribution's Vagrant, which is older than labs need (Debian 13 ships 2.3.8), so every VM lab
  then refused it. Where HashiCorp has no packages for the release, the setup says so and links
  to Vagrant's install page.
- **On Linux, an attack box with the shared folder starts on libvirt.** The libvirt install now
  brings `virtiofsd` where the release packages it; libvirt's shared folders need it, and the
  attack box failed to start without it.
- **On a bare Linux the setup installs curl first.** Debian's minimal image has no curl, so the
  Vagrant install failed saying HashiCorp had no packages for the release, and the other vendor
  installs failed the same way. A vendor repository that can't be reached is now said as such,
  not as a release it doesn't serve, and Vagrant's refusal names Vagrant's install page, not
  Terraform's.
- **The attack box is added with a libvirt lab.** At the end of a launch, the lab page's status
  read and the attack box's ran two `vagrant status` of the same VMs at once; Vagrant refused one
  (its machine index was locked) and the box "couldn't be added with the lab". A run's status is
  now read one read at a time, and no Vagrant runs while an operation holds the run.
- **A cancelled cloud sign-in says it was cancelled, with Retry.** For AWS, Azure and Google
  Cloud, a CLI sign-in the player cancelled read like a failed one, its log left on screen; it
  now hides the log, says "Sign-in cancelled" and offers Retry, while a sign-in that failed keeps
  its log and why.
- **The Azure VM size check tells "not offered in this region" from "refused to your
  subscription".** It names the regions where the size can be had (never the one checked) or,
  for a size refused almost everywhere, the sizes the subscription can use there, and reads each
  list once a session.
- **A hypervisor chosen in Settings or the Machine setup applies in the main window at once.**
  Both are windows of their own, and the main window's header, Machine page and lab list kept the
  choice they had read until it was reopened. They now follow a change made in any window.

### Security

- **The release app no longer carries the GUI test hooks.** Two components the WebDriver tests
  use, one of which starts the lab folder a `localStorage` key names, were mounted in every
  build. They are now built only into a development frontend, or a build made for those tests
  (`NEXT_PUBLIC_WDIO=1`).
- **The development servers listen on this computer alone, and the app's serves its window
  only.** `pnpm dev`, the server `pnpm tauri dev` loads, listened on every network interface, so
  any device on the network could load the interface, Next's dev endpoints and the source maps.
  It now listens on 127.0.0.1 (`devUrl` is `http://127.0.0.1:3206`), and a browser that opens it
  gets a notice instead of a page that crashed on its first Tauri call. The browser preview on
  sample data is its own command, `pnpm dev:web` (127.0.0.1:3207, `?mock`), the only server
  built with the dev mock; Playwright runs against it. A release still has no server: the app
  embeds the static export.
- **Kali's archive keys are checked before a Debian attack VM trusts them.** Where Kali has no VM
  image for the computer's CPU, the attack VM is a Debian VM made Kali; it downloaded Kali's
  archive keyring straight into apt's keyrings, trusted on TLS alone. The keyring is now checked
  against the fingerprints of Kali's archive keys pinned in the launcher, and the set-up stops,
  installing nothing from Kali, when it holds any other key.
- **rustls-webpki 0.103.16** (RUSTSEC-2026-0382: certificate paths through distrusted
  intermediates could be accepted).
- **A chain of symlinks in a lab can no longer reach outside the lab's folder.** Each link in a
  lab's archive or imported folder was judged by its path alone, so two links that each read as
  inside the lab (`sub/a -> ..` and `b -> sub/a/../secret`) together pointed beside it, and a
  lab's files, built into its images and mounted in its machines, could read the app's data. The
  links are now made once the rest of the lab is written, and each is kept only when what it
  resolves to on disk is inside the lab's folder; a link left out is said in the log.

## [1.8.9] — 2026-11-08

### Added

- **A VM lab on this machine gets an attack VM, on QEMU too.** "Start attack box" on a lab
  running as VMs on a local hypervisor now adds an attack VM to the running lab through the
  Isothrum runner, as GNS3 adds a node to a live topology (the runner writes its definition into
  the run's state and boots it alone, through the hook every Vagrantfile Isoloom 1.0.9 writes;
  a spec's own `access` machine is used when it declares one): the panel shows its address on
  the lab network, "Open shell" is `vagrant ssh` into it, Stop destroys it alone with the lab left
  up. Until a box is picked in Settings, the lab's default box for this machine's CPU runs (Kali
  on x86, the Debian cloud image on an Apple Silicon Mac, where Kali's box has no build and
  emulated it is far too slow). On QEMU the lab's controller is no longer offered as the attacker
  in its place.

- **A container lab running in a VM on this machine gets a working attack box.** "Start attack
  box" on a lab in its helper VM (Isoloom's docker-vm, UTM included) now stands the box up as a
  container inside that VM, on the lab's network there, through the Isothrum runner (which
  refused it before: "hot-adding an ad-hoc access VM … isn't supported"); the panel shows its
  address, "Open shell" goes through the VM into it (`vagrant ssh … docker exec …`, with the run's
  state folder, which the in-app and system terminals now pass on), and Stop takes it out. On
  UTM, a hot-add that met UTM's passing "Application isn't running (-600)" is tried once more,
  as a start is.
- **A lab with both forms can run here as one VM per machine.** A container lab whose Isoloom
  spec also has a VM recipe per machine (the backend's `docker` and `vm` flags both set, e.g.
  `supplier-portal-api`) gets a third place in "Where should it run?": "This machine, one VM per
  machine", next to its containers and the one helper VM. It runs through Isoloom's vagrant
  target like a VM lab, on a ready hypervisor that form runs on (not UTM: its Vagrant plugin gives
  a VM no private network, and Isoloom refuses it there); status, stop, pause, shut down and
  provisioning follow the form the lab was started in, whatever the catalogue calls it.

### Fixed

- **"Start attack box" no longer says a running lab on UTM isn't running.** The runner read
  UTM's `started` VM state as unknown, so the box's start found no running target and failed with
  "This lab isn't running. Start it first." two seconds after the lab came up; with the runner's
  fix (`started` reads as running) and a container read in the VM that is tried once more when
  it misses, the box starts on the running lab.
- **The AWS account test confirms the launch permission instead of warning on every account.**
  Its dry run of `ec2 run-instances` used a made-up image id, which AWS refuses as malformed
  before it looks at the permission at all, so a properly signed-in account read "Needs
  attention" with `InvalidAMIID.Malformed`. The dry run now targets the region's latest Amazon
  Linux image (the SSM public parameter; a well-formed unknown id when that can't be read), and
  an answer AWS only gives once the request is authorized counts as the permission passing.
- **"Connect" on the Cloud page's Azure and Google Cloud rows opens the setup on that provider.**
  It opened "Set up cloud provider" at "Choose a cloud provider" with Amazon Web Services picked
  whatever row was clicked. The click is the choice: the setup now skips that step and opens on
  the row's provider, at its tools and sign-in. "Set up cloud provider" at the top keeps the full
  flow.
- **A lab on UTM waits for UTM to be open and answering before `vagrant up`.** Vagrant's UTM
  plugin reads the VM's state and its forwarded ports from the UTM app through Apple events, and
  a UTM not running yet, or one macOS doesn't let Cyber CTF control, failed the start mid-way
  with "Application isn't running (-600)". UTM is now opened first and waited for; an Automation
  refusal is named with its way out.
- **Setup checks the versions of UTM and its Vagrant plugin, not only their presence.** A
  `vagrant_utm` older than 0.2.0 reads as not installed, so setup installs it again (which
  updates it), and a UTM older than 4.6.5 (the oldest the plugin drives) is named as too old.
- **A UTM start is tried once more on UTM's passing "Application isn't running (-600)".** UTM
  answers that to a VM customization right after an import now and then, while it runs and
  answers everything else; the half-created VM is removed and the start runs again, instead of
  failing the lab on a one-off.
- **A container lab in a VM stays known as such after a retried start.** Removing the
  half-created VM of a failed first attempt also forgot that the lab runs in a local VM, so once
  the retry succeeded the page read it as plain Docker: not running, no machines, "Shut down"
  offered, the attack box refusing. Each attempt marks it again; the page and the Deployment
  panel now also say "a VM on this machine" for such a lab, whatever the picker last held.
- **Pause, Resume, Stop and the attack box wait for the lab's own checks instead of failing.**
  The page's periodic status check holds the run for a few seconds, and a Pause clicked meanwhile
  failed with "Another operation on this lab is already in progress". The player's operations now
  wait up to a minute for the run to be free.
- **The Machine page's Setup summary names the hypervisor chosen in Settings.** It named the
  first installed one (QEMU) while the header, the Health view and the labs used UTM.
- **Switching the hypervisor is one click.** The hypervisor chosen in Settings never reached the
  header, which named the first ready one (QEMU before UTM), and the Machine setup's pick wasn't
  saved as the choice at all. Both follow the choice now, and "Where should it run?" carries a
  switch inside its "in a VM" option when several hypervisors are ready; its pick is saved as
  the choice.

## [1.8.8] — 2026-11-07

### Fixed

- **A lab whose launch failed no longer shows as running.** A failed launch can leave a lab's
  machines up; the page then showed the "Running" badge and a full, healthy topology diagram even
  though the deployment had failed. A failed launch now reads as not running for local labs too (it
  was masked only for server and cloud labs), so the page shows "Failed to start", no diagram, and
  routes to the Stop & clean up path.
- **QEMU docker-in-VM labs no longer collide on the helper VM's SSH port.** Bundles Isoloom 1.0.8
  and the Isothrum runner 1.0.4: the QEMU helper VM's SSH forward takes a free host port picked
  ahead of time, and the cloud-docker `AcceptEnv` drop-in directory is created before it is written.

## [1.8.7] — 2026-11-06

### Fixed

- **"VM on this machine" is no longer offered for a container lab on a Hyper-V-only machine.**
  Running a container lab in a VM (docker-in-VM) is not supported on Hyper-V, so offering it there
  only failed at launch; the place now reads as unavailable where Hyper-V is the only hypervisor.

## [1.8.6] — 2026-11-06

### Fixed

- **A container lab runs as a VM on QEMU, not only VirtualBox/VMware/libvirt.** Bundles Isoloom
  1.0.7: the docker-in-VM helper box is now `generic/debian12`, which has a build for every local
  hypervisor, so "VM on this machine" works on a QEMU-only machine. (Not on Hyper-V, which
  Isoloom's docker-in-VM doesn't support: 1.8.7 stopped offering it there.)

## [1.8.5] — 2026-11-06

### Fixed

- **A container lab whose ports are all fixed starts with "the lab's default ports".** Choosing
  that option passed `ISOLOOM_PUBLISH_FIXED`, which an all-fixed lab's manifest did not declare, so
  the start failed with "isn't a param of this manifest". Bundles Isoloom 1.0.6, which declares it
  for any lab that publishes a port.
- **A cloud lab delivers its launch values to the containers.** A cloud start failed with
  `CTF_LAUNCH_TOKEN ... is not set`; Isoloom 1.0.6 now has the cloud VM accept the forwarded values.
- **"VM on this machine" is offered on any ready hypervisor.** A container lab runs as docker-in-VM
  inside a generic helper VM, so it no longer needs its own provider list to include the hypervisor
  you have ready (e.g. QEMU): the place reads as ready and the launch targets it.

### Changed

- **"Where it runs" colours only the icon** of a place you could set up (the label and hint stay
  neutral), the "default ports" warning is one line, and the "connect an account" hint no longer
  shows a literal `{clouds}`.

## [1.8.4] — 2026-11-05

### Fixed

- **The VM self-test now boots on QEMU.** It fetched its test box with `vagrant box add --provider
qemu`, but `generic/alpine319`'s qemu box is a libvirt image mislabeled, so the add failed
  ("Provider of box: libvirt"). The test now adds and lists the box under the libvirt provider for
  QEMU: the vagrant-qemu plugin boots a libvirt box's disk directly, and that box bakes in the
  vagrant user, so the VM comes up with working SSH without cloud-init (which vagrant-qemu does not
  seed). This mirrors how the Isoloom engine runs its QEMU labs.

## [1.8.3] — 2026-11-05

### Added

- **"Where it runs" shows what you could set up.** A place the lab supports but your launcher
  isn't configured for — a server with none connected, a cloud account with none connected, or a
  VM with no local hypervisor installed — now shows in amber with a short hint ("Connect a
  server", "Connect an account", "Install a hypervisor"), distinct from a place the lab can't run
  on at all.

### Changed

- Bundles Isoloom 1.0.5 and Isothrum 1.0.3: a lab run as a VM on this machine now gets a free
  host port picked for each published port instead of its own fixed number, so a lab that
  publishes a privileged port (80/443) starts as a VM without administrator rights and two labs
  that publish the same port no longer collide.

## [1.8.2] — 2026-11-05

### Added

- **The website can tell which launcher each machine runs.** When it registers and on every
  heartbeat, the launcher now reports its own version and the machine's OS (e.g. `macos 15.1`,
  `windows 11`), so an outdated launcher can be spotted. The OS falls back to its family alone
  (`linux`) when its version can't be read.

### Removed

- **"Hosted by Cyber CTF" is gone.** Running a lab hosted by Cyber CTF is no longer offered as a
  place to run it; a lab runs on your own machine, on a server, or in your cloud account.

## [1.8.1] — 2026-11-05

### Fixed

- **A port clash now names what is holding the port.** When a lab can't start because a host
  port is already taken, the message says which program holds it and its process id, so you can
  stop that program and start the lab again.
- **"Retry with random ports" no longer appears for a lab run as a VM on this machine.** A VM
  forwards the lab's fixed ports to this machine, so random ports can't move them; freeing the
  port is the fix. The button also no longer quietly turns a VM start into a plain container one.

## [1.8.0] — 2026-11-05

### Added

- **A lab can be run more than one way, and you choose.** When a lab's spec describes both a
  container form and a VM form, the launcher now offers a place for each — as containers, as one
  VM per machine locally, on a server, or in the cloud — instead of only the single way it was
  published with.

### Fixed

- **The system-requirements line works on every platform.** The `lab_requirements` command was
  missing from the app's permission manifest, so the pre-launch "Needs" panel could fail to load;
  it is now granted. French, Spanish, German, Portuguese (Brazil) and Japanese also gain the
  requirements strings that were English-only.

### Changed

- Bundles Isoloom 1.0.4: a lab's `setup:` step now runs with the project mounted at the same path
  on every target, so one spec sets a dual-form lab up identically as a container or a VM.

## [1.7.0] — 2026-11-03

### Added

- **A lab now shows what it needs to run.** The pre-launch panel gains a "Needs" line with the
  minimum CPU, memory and disk, as a range from the lab's lightest form (its machines as
  containers) to its heaviest (one VM per machine). It is read from the lab's own spec, so it is
  right for each lab.

### Fixed

- **UTM no longer opens on its own.** The running-labs scan asked `utmctl` on every pass, and on
  macOS that launches the UTM app when it isn't already open. It now asks only when UTM is already
  running.
- **Installing QEMU on an Apple Silicon Mac no longer fails** with "unknown or unsupported macOS
  version". Homebrew is updated first, which an older Homebrew on a newer macOS needs before it can
  install anything.

### Changed

- Bundles Isoloom 1.0.3 (per-target resource sizing, behind the new requirements line).

## [1.6.0] — 2026-11-03

### Changed

- **A smoother sign-in.** Signing in now brings the launcher back to the front
  once the browser step is done and resumes whatever asked you to sign in, with
  a real waiting state while it completes. An expired session is called out
  plainly, your account picture is shown once you are in, and the sign-in page
  matches the launcher's own look; the browser tab it opens closes itself.

### Security

- Updated Next.js to 16.4.3.

## [1.5.3] — 2026-10-28

### Fixed

- **The on-demand attack box no longer shows as a second, unnamed "running
  lab" in the sidebar.** The attack box runs as a container named
  `cyberctf-<id>-isoloom-access` (no Compose project), which the sidebar's
  workload scan read as a separate lab id and displayed by its raw id
  (e.g. "51bb8765 96d7 49d6 862c …") instead of grouping it under its lab. It
  now groups correctly, so only the lab itself is listed. The main "Running
  now" panel was never affected.

## [1.5.2] — 2026-10-28

### Changed

- Bundles Isoloom 1.0.2. A lab may now publish a pre-built Docker image it also
  builds (the Compose build+image combo): the launcher uses the published image
  when it can pull it and only builds locally when it cannot, so those labs start
  without a local build. Also brings libvirt boxes for Windows Server 2016/2019,
  so those Windows VM labs deploy on vagrant-libvirt (KVM).

## [1.5.0] — 2026-10-18

First public release: a thin desktop shell over the Isothrum runner.

### Added

- Runs every lab — on the player's own machine (Local), on a server (Server) or
  in a cloud account (Cloud) — through the Isothrum runner. The launcher keeps
  no lab runtime of its own; Isoloom reads each lab and converts it to the right
  infrastructure files, and Isothrum runs them.
- The lab's on-demand attacker box is the runner's access machine, hot-added
  into and removed from a running lab. There is no separate attacker runtime.
- Bundles Isoloom and Isothrum 1.0 and reads the 1.0 spec format only.
- Engine-compatibility gating: a lab is shown only when its format and its
  minimum engine versions match the bundled Isoloom and Isothrum.
- Per-objective flags are delivered to the lab as files, never as environment
  variables.
