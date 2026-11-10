# Changelog

All notable changes to Cyber CTF Desktop are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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
- **The attack box on every form a lab runs in.** The attack box is one more machine of the lab,
  wherever it runs: on Docker, in a VM on this machine, as VMs here, on a Proxmox server, or in a
  cloud account (containers on one VM, or one VM per machine). Beside containers it is a container
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

### Removed

- **The attack VM box in Settings.** It chose the Vagrant box of a lab's own attack VM; the
  attack box among VMs is now always Kali, made by the launcher.

### Changed

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
  on macOS keeps a run's `no_proxy` entries instead of replacing them.
- **No vendor install script runs as root on Linux.** The one-click installs of Azure CLI and
  gcloud set up Microsoft's and Google's apt repositories, and VirtualBox's Oracle repository, each
  with its signing key checked against a fingerprint pinned in the app, as Docker's and
  HashiCorp's already were; Docker on a distro without Docker's apt repository says where to get
  it instead of running get.docker.com. gcloud is now a system package (`google-cloud-cli`)
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
- **The Machine page lists its providers, and "Add a provider" opens a guided setup per lab
  type that starts from where the machine stands.** The Health/Setup switch and the setup
  checklist are gone: the page shows this machine's health and every container engine and
  hypervisor found on it as a row, each with its own state (the machine check now asks every
  Docker CLI context in turn with `docker --context <c> info`, so Docker Desktop, OrbStack,
  Colima, Docker Engine or Podman side by side each read running or stopped, not only the one
  the CLI points at; the current engine and the hypervisor labs run on are marked). Every
  running engine and every ready hypervisor has its own Test: the container test runs on that
  engine's Docker context, the VM test on that hypervisor, and each row keeps its own last
  result ("tested 5 min ago", or failed and why), the lab type's own readiness following only
  the engine or hypervisor labs run on. "Use by default" on a row points Docker at that engine
  or makes that hypervisor the Settings choice. Only what is installed is listed, in the Providers panel and in
  Details alike: nothing for the engines and hypervisors this machine lacks. Adding one is what
  "Add a provider" is for: in the header as the Servers page's "Add server", it opens the guided
  setup on a choice of the providers this machine can have, a container engine or a hypervisor
  for VMs, each row saying how far along the machine is. The setup is about this machine only
  (cloud accounts and servers have their own pages). A path lists only its own steps and opens
  at the first one still to do, read from the machine check and the last self-tests: a machine
  whose engine is installed and tested goes straight past the Docker steps to its summary,
  instead of walking them again. A row's Fix or Test still opens its step directly.

### Fixed

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
- **A cloud-services lab gets the account's region, subscription or project.** The lab's own
  Terraform module was never given where the account works when its provider names none: the AWS
  region (`AWS_REGION`), the Azure subscription, the Google Cloud project and region. They are now
  given from the picked account, unless the lab's `cloud.env` sets them, and a lab brought over
  from an earlier version keeps the region it was applied in.
- **A lab whose state can't be read says why.** When reading a lab's state failed (Docker not
  running, its run record unreadable, its files no longer able to be generated again), the lab
  page went on showing the last state it had read, with no word of it. It now says, at the top,
  that the state couldn't be read and why, until a read succeeds.

### Security

- **rustls-webpki 0.103.16** (RUSTSEC-2026-0382: certificate paths through distrusted
  intermediates could be accepted).

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

- **A container lab runs as a VM on QEMU and Hyper-V, not only VirtualBox/VMware/libvirt.** Bundles
  Isoloom 1.0.7: the docker-in-VM helper box is now `generic/debian12`, which has a build for every
  local hypervisor, so "VM on this machine" works on a QEMU-only (or Hyper-V) machine.

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
