# Desktop end-to-end

The real app, driven through its embedded W3C WebDriver server (`tauri-plugin-wdio-webdriver`,
built in with `--features wdio`, never in a release build): no external driver, so the same specs
run on Linux, Windows and macOS. One entry point a system:

```sh
tests/e2e/desktop/runner/run.sh --all                                   # Linux, macOS
tests/e2e/desktop/runner/run.sh --group lab                             # one group of specs (below)
tests/e2e/desktop/runner/run.sh --specs "smoke settings" --knobs ~/e2e.env
tests/e2e/desktop/runner/run.sh --app path/to/cyberctf-desktop          # an app already built
tests/e2e/desktop/runner/run.sh --package path/to/Cyber.CTF_1.8.9_amd64.deb   # .deb, .rpm or .AppImage, unpacked
```

```powershell
pwsh -File tests\e2e\desktop\runner\run.ps1 -All                        # Windows, in the desktop session
pwsh -File tests\e2e\desktop\runner\run.ps1 -Group lab
pwsh -File tests\e2e\desktop\runner\run.ps1 -Specs "smoke settings" -Knobs $HOME\e2e.env
pwsh -File tests\e2e\desktop\runner\run.ps1 -Package path\to\Cyber.CTF_1.8.9_x64_en-US.msi   # or the NSIS setup .exe
```

Both are thin: they build or unpack the app and serve a debug build's frontend, then start
`tests/e2e/desktop/runner/run.mjs`, the runner itself, the same on every system (it makes the run's folder,
checks the needs, runs the specs and writes the results). Without `--app` or `--package` they build the app with `cargo build --features wdio`,
keep a copy of it as the runner's own (`<target>/debug/wdio/cyberctf-desktop`, below), and
serve the frontend such a build loads (`next dev` on 3206, `tauri.conf.json`'s `devUrl`; an app
from `pnpm tauri build` embeds `out/`, a `cargo build --release` doesn't). A package must be a
`tauri build` with the hooks: `NEXT_PUBLIC_WDIO=1 pnpm tauri build --features wdio`
(the `NEXT_PUBLIC_WDIO=1` keeps the `__wdio_*` hooks a few specs use). On Linux the runner always starts
Xvfb for the run, whatever display the shell has (an SSH session has none; a desktop's terminal or a
VNC session's has theirs), on a display number no other X server holds (`:99` and up), with a
light window manager on it, the first installed of `xfwm4`, `openbox` and `fluxbox`, and a D-Bus
session bus of the run's own (`dbus-daemon`). It gives its `DISPLAY` and that bus's
`DBUS_SESSION_BUS_ADDRESS` to the specs and their apps, with `GDK_BACKEND=x11` and without the
shell's `WAYLAND_DISPLAY` and `AT_SPI_BUS_ADDRESS`, and stops all three however the run ends (their
output in `<logs>/xvfb.log`): who starts the run, and from where, doesn't change its results. The
desktop session's bus belongs to another display: a service there that never answers (a broken
`xdg-desktop-portal`) holds every GTK app's start for D-Bus's 25 s timeouts, long enough that the
app's page never loads in time. Without Xvfb or `dbus-daemon` installed it stops at once and names
the package (`xvfb`, `dbus-daemon`) rather than letting the app panic or reach the session's bus. A bare
Xvfb has no window manager to act on a maximize or a minimize: without one installed, the run goes
on and the `title-bar` case skips those checks, saying why.
Before the specs, the runner starts the app once and stops with a clear message when no WebDriver
server answers. Before it ever starts the app, it checks the app is a `wdio` build, below. It checks out
the fixture lab (the Invoice Portal API lab, under `tests/e2e/desktop/.state/labs`) unless `CYBERCTF_TEST_LOCAL_LAB` names one.

It is safe on a machine someone uses: the app keeps all of its own (data, settings, logs, webview
data, session, secrets) in a folder made for the run and removed after it, and every clean-up
removes only what the run made, by the records it wrote: never the user's labs, images, boxes or
apps ([tests/README.md](../../../tests/README.md#a-desktop-runs-own-folder)). Before that folder goes, the runner stops
the Isothrum runner services the run's apps started (`<app> serve --home <home> --detach`, which
outlive the app), those whose home is in the folder only.

### Only a wdio build is started

Only a `wdio` build keeps to the run's folder: any other build ignores `CYBERCTF_WDIO_ROOT` and
uses the user's own data folder (on macOS `~/Library/Application Support/org.cyberctf.desktop/`),
their labs, settings and runner service. Three things keep a run from ever starting one:

- **The runner reads the app's file first.** A `wdio` build's binary holds a mark no other build
  has (`src-tauri/src/platform/test_root.rs` `BUILD_MARK`, `tests/e2e/desktop/runner/preflight.mjs`
  `WDIO_BUILD_MARK`). The runner looks for it in the file, never by starting the app, before it
  makes the run's folder, and again before each spec when the file changed (inode, size, time).
  Without it the run stops with exit 2 (or, mid-run, that case fails and the rest skip), naming the
  file and how to rebuild; the app is never started. For an AppImage, whose binary is compressed,
  `run.sh` extracts the binary with the AppImage's own `--appimage-extract` and the runner checks
  that (`CYBERCTF_TEST_DESKTOP_BINARY`).
- **The runner drives a copy of its own.** `cargo test` and a `cargo build` without the feature
  relink cargo's `<target>/debug/cyberctf-desktop`, the file the wdio build was in, so a run
  started after them, or a spec started while they ran, would drive a build that ignores the run's
  folder. `run.sh` and `run.ps1` copy the wdio build they just made to
  `<target>/debug/wdio/cyberctf-desktop` (`support/host.mjs` `keepWdioBuild`: written beside it,
  then renamed over it), which nothing else writes, and drive that; `--skip-build` drives that
  copy as the last build left it. A copy rather than a target directory of its own: the two builds
  differ only in the app's crate and the two WebDriver plugins, so they share every other compiled
  crate in one target directory; a second one would build the whole tree again and hold as much
  again on disk. The copy keeps the binary's name (macOS names a bare binary's WebKit folders
  after it) and stays in a debug folder (the run serves it `devUrl`'s frontend).
- **The app refuses a folder it won't keep to.** Started with `CYBERCTF_WDIO_ROOT` set, a build
  without `wdio` (or one given a folder that isn't absolute) stops at once with exit 2, before it
  reads or writes any folder, saying how to build it (`test_root::refusal`). A player never sets
  the variable: their app starts as before.

## Results

Each spec prints a line per check, as the integration tests do (`integration_tests/support`'s `check`), with a
stable id and the seconds since the line before:

```text
ok: maximize (2s) Maximize maximizes the window
FAILED: whoami (20s) whoami in the shell answers player: no answer to `whoami`; ...
skipped: vm-test (0s) no hypervisor asked: set CYBERCTF_TEST_VM_PROVIDER
```

A spec exits 0 when every check passed or was skipped, 1 when one failed, 2 when it skipped as a
whole (`skipped: spec (0s) <why>`: a knob it needs is missing). The runner shows the check lines
as each spec ends and prints the summary of `scripts/lib/suite.sh`, a line per spec, then one per
check, named `<spec>/<check>` (counted once, under the spec):

```text
   title-bar            ok        41s  tests/e2e/desktop/.state/logs/title-bar.log
   title-bar/controls   ok         3s  the main window's header ends with Minimize, Maximize and Close
   lab-form-vm              skip       0s  no hypervisor named: set CYBERCTF_TEST_VM_PROVIDER (...)
```

With `--results <file>` (`-Results` on Windows) the runner also appends a JSON record per spec and
per check there, and `--list` (`-List`) prints its cases, each with its knobs, groups and needs
([tests/README.md](../../../tests/README.md#results)).
Its exit status: 0 nothing failed, 1 a spec failed, 2 the runner itself couldn't run; `STRICT=1`
fails a skip. Logs: `tests/e2e/desktop/.state/logs/<spec>.log`, and the apps' own logs under
`tests/e2e/desktop/.state/logs/app/` (copied from the run's folder before it goes, without the lines
that may name a secret).

## Needs, caps and groups

A failure costs seconds, not an hour. Each spec declares what it needs on its first lines
(`export const needs = [...]`, read by `tests/e2e/desktop/runner/plan.mjs`), and the runner meets each
need once:

- The runner provides some itself, checked when the first spec needing it comes up: `engine` (a
  Docker engine answers, within a minute), `fixture-lab` (the lab folder), `session`
  (`CYBERCTF_TEST_ACCOUNT_SESSION`), `hypervisor` (`CYBERCTF_TEST_VM_PROVIDER`, and its tool answers
  where one can be asked), `server` (`CYBERCTF_TEST_SERVER_ENV`), `own-frame` (Linux, Windows),
  `unix`.
- A spec provides the others by passing (`export const provides`): `launch` provides
  `running-lab:fixture` (the fixture lab starts here), `lab-form-container` provides
  `catalogue-lab` (a catalogue lab starts here, signed in), `lab-form-per-machine` provides `vm-start`
  (a VM lab starts on the hypervisor).

A need not met skips every spec needing it at once, in the console and the results, with the
failure as the reason (`needs running-lab:fixture, which launch didn't provide (failed: the seeded
lab never showed as running on Overview (exit 1))`); the spec that owns the failure keeps its
own verdict, with the app log's last lines. So the twelve specs that start the fixture lab wait
for one failed start, not twelve; the three VM forms and `screens` for one VM that can't start.

Every spec is killed past its cap, 5 minutes (`CYBERCTF_TEST_DESKTOP_TIMEOUT`; `machine` gets 8),
and keeps its own waits under it (`support/app.mjs`'s `LAB_START_MS`, 3 minutes for a lab to start,
`VM_START_MS`, 4 for a VM lab, `STOP_MS`, 2 for a stop, each a fraction of the cap), so a wait
that runs out fails with its own reason first. An app that dies under a spec (WebDriver
`ECONNREFUSED`) is started again once, to see whether it comes up at all; a second death, or none,
ends the run with that reason, the specs left skipped.

One app at a time answers on the WebDriver port (`TAURI_WEBDRIVER_PORT`, 4445). Each spec runs in
a process group of its own, ended with all it holds (its apps, whatever they started) when the spec
exits, at its cap, or when the run itself is stopped (SIGINT, SIGTERM, SIGHUP; on Windows
`taskkill /T`). Before each app, the runner's and each launch in a spec, the port is waited for,
10 s at most (`support/host.mjs`): an app that can't bind it runs on without its server, and the spec
would drive whatever still answers there, another copy of the app among them, without its own
knobs. A port still held fails the case (`the previous app still holds port 4445 (pid …: …)`) and
ends the run, the cases left skipped; held before the run starts, it refuses to run.

The cases fall into four groups (`cases.tsv`'s last column; `--group <name>`, `-Group`), for a
caller that runs them as parallel jobs, each on a machine of its own; a group holds the providers
of what its specs need, so `launch` is in two:

| Group        | Cases                                                                                                                                             | About                                           |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------- |
| `shell`      | smoke, title-bar, settings, onboarding, setup-screens, labs, deep-link, fail-offline, fail-badlab, fail-signedout, fail-docker, attack-on-stopped | no lab, but `labs`' import                      |
| `lab`        | launch, restart, stop, launch-ui, attack-box, attack-shell, stop-with-box                                                                         | the fixture lab's life                          |
| `resilience` | launch, kill-mid-deploy, stop-during-start, double-start, close-mid-deploy, kill-runner-service, park-resume, reimport-running                    | what breaks a start                             |
| `vm`         | machine, lab-form-container, lab-actions, offline-start, lab-form-per-machine, lab-form-vm, screens, server-lab, cleanup                          | signed in, a hypervisor, a server; cleanup last |

`--all` runs the groups in that order, each case once. A group that runs on another machine
than the rest writes its own results file; `launch`'s record then appears in two of them.

## Knobs

From the environment or the `--knobs` file (`KEY=value` lines, read literally, never sourced,
as a harness writes its settings files). [tests/README.md](../../../tests/README.md#knobs) lists
each, with the specs that read it.

## Feature map

Every feature a player uses, the spec that drives it, what it needs, and what is left to the
integration tests (`docs/contributing/testing/integration.md`) or the unit tests. Cloud accounts are out of
this layer: the integration tests' AWS tier covers them.

| Feature                                                                         | Spec                                                                                                | Needs                                                                      | Left to the integration or unit tests                                                    |
| ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| First run: the onboarding wizard                                                | `onboarding`                                                                                        | nothing                                                                    | each machine step's installs (bare-machine tier)                                         |
| Settings: theme, language, shared folder, attack VM desktop                     | `settings`                                                                                          | nothing                                                                    | the box seeing the folder on every form (`attack-box-*`)                                 |
| The Machine page: providers, load, self-tests                                   | `machine`                                                                                           | Docker for the container test; `CYBERCTF_TEST_VM_PROVIDER` for the VM test | each hypervisor's own probe (unit tests)                                                 |
| The guided setup (from the Overview, and Add a provider)                        | `machine`                                                                                           | nothing (opened with Docker made unreachable)                              | installing the tools on a bare machine: the bare-machine tier                            |
| The labs list, its search, a catalogue lab's page                               | `labs`                                                                                              | the catalogue reachable                                                    |                                                                                          |
| Importing a lab, the imported labs and their page                               | `labs`, `launch-ui`, `reimport-running`                                                             | `CYBERCTF_TEST_LOCAL_LAB`, Docker                                          | `replace-*` (a running lab never replaced)                                               |
| A local lab as containers                                                       | `launch`, `restart`, `stop`, `lab-form-container`                                                   | Docker; signed in for `lab-form-container`                                 | `docker-lab-hello-stack`, `docker-lab-segmented` (checks, park and resume per form)      |
| A VM lab                                                                        | `lab-form-vm`                                                                                       | signed in, `CYBERCTF_TEST_VM_PROVIDER`, `CYBERCTF_TEST_CATALOGUE_VM_LAB`   | `vm-lab`                                                                                 |
| One VM per machine                                                              | `lab-form-per-machine`                                                                              | signed in, `CYBERCTF_TEST_VM_PROVIDER`                                     | `per-machine`                                                                            |
| A VM's Screen window (VNC framebuffer, RDP hand-off, no screen and why)         | `screens`                                                                                           | signed in, `CYBERCTF_TEST_VM_PROVIDER` (libvirt or QEMU give VNC)          | each runner's console answer and the bridge (unit tests); a server's or a cloud's screen |
| The attack box: added, its shell window, `whoami` is `player`                   | `attack-shell` (the lab page), `attack-box` (the panel alone), `attack-on-stopped`, `stop-with-box` | `CYBERCTF_TEST_LOCAL_LAB`, Docker                                          | the box on a VM lab, in the lab's VM and on AWS (`attack-box-*`)                         |
| The attack box's desktop and its Screen                                         | `settings` (the choice), `screens` (its Screen)                                                     | `CYBERCTF_TEST_BOX_DESKTOP=1` for the Screen                               | Kali's desktop set-up itself (`attack-box-vm`)                                           |
| Pause, Resume, Reset, Stop & remove (system dialogs)                            | `lab-actions` (Cancel then Remove), `labs` (imported lab), `park-resume`                            | signed in, Docker                                                          | park and resume on the VM forms (`*-lab`)                                                |
| Offline start                                                                   | `offline-start`                                                                                     | signed in, Docker                                                          | a reachable backend never bypassed (unit tests)                                          |
| Servers: add Proxmox or ESXi, its test, a lab there                             | `server-lab`, `setup-screens`                                                                       | `CYBERCTF_TEST_SERVER_ENV`; signed in for the lab                          | `server-lab-proxmox`, `server-lab-esxi` (status, checks over SSH, provisioning)          |
| Cleanup: downloads and installed tools                                          | `cleanup`                                                                                           | `CYBERCTF_TEST_CLEANUP_TOOL` to remove a tool                              |                                                                                          |
| The title bar and window controls (Linux, Windows)                              | `title-bar`                                                                                         | Linux or Windows (macOS keeps the system's frame)                          |                                                                                          |
| Deep links (`cyberctf://labs/<slug>`)                                           | `deep-link`                                                                                         | the catalogue reachable, or an imported lab                                | the system routing the scheme to the app (the packaging smoke)                           |
| Failure states: offline, signed out, no Docker, a bad folder, a kill mid-deploy | `fail-offline`, `fail-signedout`, `fail-docker`, `fail-badlab`, `kill-mid-deploy`                   | Docker for the last                                                        |                                                                                          |

What the specs answer in place of the system, and why: the system's own dialogs (a destructive
action's confirmation, the folder and file pickers) are windows WebDriver can't press, so
`confirmSystemDialogs` and `answerPickers` answer the webview's dialog call before it reaches the
core, Remove or Cancel, the path given; everything after that call is real. A link opened while the
app runs is handed over as the deep-link plugin hands it (its `deep-link://new-url` event); the
link the app is started with goes through the plugin's own command-line reading. In the shell
window, `whoami` is typed into the terminal's input; when the webview's synthetic keys don't reach
xterm.js, the spec writes the same line through the command the terminal sends keys with, and says
so in the check line.

## The specs

| Spec                | Proves                                                                                                                                                                                                                                       |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `smoke`             | the app boots and its shell renders                                                                                                                                                                                                          |
| `title-bar`         | Linux and Windows: Minimize, Maximize, Restore and a double-click on the header do what they say (`is_maximized`, `is_minimized`); Settings' own bar closes its window; Close on the main window quits the app                               |
| `settings`          | Settings opens in its window; the theme and the language apply there and in the main window; a folder picked is the shared folder; the attack VM's desktop choice is saved; all put back                                                     |
| `onboarding`        | the first-run wizard renders in the main window, advances, and Skip setup lands on the shell                                                                                                                                                 |
| `setup-screens`     | the Servers and Cloud screens render with their setup entry points                                                                                                                                                                           |
| `machine`           | the Machine page's panels; the container lab test (and the VM lab test) from a provider's row passes; Add a provider opens the setup window; with Docker unreachable, the Overview opens the guided setup, which leads into a path           |
| `labs`              | the catalogue is listed and searched; a catalogue lab's page; Import a lab (the picker answered) starts the lab, which is listed under Imported from a folder, opens its page, runs, and stops with Stop & remove                            |
| `deep-link`         | a link the app is opened with, and one opened while it runs, show the lab's page without starting it; a link to no lab, or not to a lab, leaves the app as it was                                                                            |
| `fail-offline`      | with the backend unreachable (`CYBERBACKEND_URL` dead), the catalogue degrades and the app stays usable                                                                                                                                      |
| `fail-badlab`       | a folder that is not a lab is refused without a crash or a phantom running lab                                                                                                                                                               |
| `fail-signedout`    | signed out (a run folder of its own with no session in it, `CYBERCTF_WDIO_ROOT`), the app is usable and asks to sign in                                                                                                                      |
| `fail-docker`       | with Docker unreachable (a dead `DOCKER_HOST` and a `DOCKER_CONFIG` naming no current context), the header says Docker is not running, the seed lab's start fails and nothing runs                                                           |
| `launch`            | the seeded local lab reaches "running on this machine" on the Overview                                                                                                                                                                       |
| `restart`           | after the app is killed and reopened without the seed, the running-labs scan finds the lab again                                                                                                                                             |
| `stop`              | Stop in Machine > Running now stops and clears it                                                                                                                                                                                            |
| `launch-ui`         | the import path (`__wdio_launch_local__`) reaches running                                                                                                                                                                                    |
| `kill-mid-deploy`   | killed mid-deploy, the app reopens coherently                                                                                                                                                                                                |
| `attack-box`        | the attack box panel alone attaches the box, gets an address, opens a shell and detaches                                                                                                                                                     |
| `attack-shell`      | on an imported lab's page: the box runs with an address, its shell window opens, `whoami` answers `player`, a file in the shared folder is read inside the box, Stop attack box removes its container                                        |
| `stop-with-box`     | stopping a lab while its box is attached leaves no container of the lab behind, the box included (leaked in 1.5.3)                                                                                                                           |
| `park-resume`       | a running container lab pauses (its containers frozen), still reads paused once the app opens again, resumes as it was (the box's shell answers), shuts down and resumes                                                                     |
| `reimport-running`  | importing a lab that is already running leaves exactly one copy and a usable app                                                                                                                                                             |
| `attack-on-stopped` | the attack box panel on a lab that isn't running shows it isn't, never a box up                                                                                                                                                              |
| `lab-actions`       | a catalogue container lab: Pause and Resume; a container stopped behind its back shows as down and Reset lab starts it again; Stop & remove answered Cancel keeps it running, answered Remove leaves nothing                                 |
| `offline-start`     | a lab started online, then started with the backend cut (through the spec's relay), is offered an offline start, runs offline, and stops                                                                                                     |
| `lab-form-*`        | `lab-form` with `CYBERCTF_TEST_LAB_FORM`: a catalogue lab started in that form through the "Where should it run?" dialog runs (asked of Docker or the hypervisor), its address answers, and Stop & remove leaves nothing                     |
| `screens`           | a running VM lab's Screens panel; a VNC screen's window shows its framebuffer; an RDP one's window hands it to the system's client; a machine without a screen says why; with `CYBERCTF_TEST_BOX_DESKTOP=1`, the attack box's desktop Screen |
| `server-lab`        | Add server, the setup window filled from the settings file (tools installed from it when missing), Save and test passing, the VM test, the server listed and Online, a catalogue lab started, running and removed there, the server removed  |
| `cleanup`           | Settings > Maintenance: Clean up answered Cancel removes nothing; answered Remove, when every download it lists is the run's, says what it freed (otherwise the others are named and all left); a named tool the run's app installed removed |

The specs are `tests/e2e/desktop/specs/<name>.mjs` and share `tests/e2e/desktop/support/app.mjs`. A case's
id is its spec's name, which its summary line and its log (`<id>.log`) carry; a spec run once per
form adds the form: `lab-form` runs as `lab-form-container`, `lab-form-vm` and
`lab-form-per-machine`. A new spec is a short `.mjs` there (its checks through `results()`, its
skips through `skipAll`, its needs declared on its first lines, its waits under the cap) plus a
row in `cases.tsv` (its id, spec, form, knobs and groups) and a row above. A spec never sleeps a
fixed time: it waits on what it expects (`waitUntil`, `waitAwake`, `eventually`, with a timeout and
a message that says what was awaited), and what must not happen it watches for the whole time
the app has to do it (`holdsFor`), not at one instant after a pause.

## On a test machine

A caller that runs the set on a fresh Linux or Windows machine runs it in the logged-in desktop
session on Windows (a scheduled task of that account). A Linux machine needs no session: the runner
always starts its own Xvfb and session bus, so the machine needs the `xvfb` and `dbus-daemon`
packages, and a window manager is recommended (`xfwm4`, `openbox` or `fluxbox`: without one the `title-bar` case's
maximize and minimize checks are skipped), besides the [Tauri prerequisites](https://tauri.app/start/prerequisites/#linux) (`libwebkit2gtk-4.1-dev`
and the rest), Node, pnpm and Rust. Its knobs file goes anywhere outside the repository, mode
600, `~/e2e.env` say:

```sh
CYBERCTF_TEST_VM_PROVIDER=libvirt            # hyperv or virtualbox on Windows
CYBERCTF_TEST_ACCOUNT_SESSION=/path/to/session.json
CYBERCTF_TEST_CATALOGUE_VM_LAB=<a catalogue VM lab's title>
CYBERCTF_TEST_SERVER_ENV=/path/to/server.env
```

Then, from the launcher's checkout:

```sh
tests/e2e/desktop/runner/run.sh --all --knobs ~/e2e.env
```

```powershell
pwsh -File tests\e2e\desktop\runner\run.ps1 -All -Knobs $HOME\e2e.env
```

Such a caller reads the records `--results` writes ([tests/README.md](../../../tests/README.md#results)):
a line per spec, and a line per check as `<spec>/<check>`; the summary on screen says the same.

## Why the runner does what it does

An app from `cargo build` needs the dev server: the frontend is embedded only with Tauri's
`custom-protocol` feature, which `pnpm tauri build` turns on and a plain `cargo build` doesn't,
`--release` or not. Without it the app loads `devUrl`, and started by hand shows a blank window
until `next dev` answers on port 3206.

The specs force the page visible: the frontend pauses its polls while the document is hidden
(`src/lib/visibility.ts`), and on macOS a window another one covers reports hidden, so a driven app
never learned that its lab came up. `support/app.mjs` overrides `document.visibilityState` and
re-applies it on every wait tick (a reload drops it).

The `__wdio_*` hooks (`src/components/wdio-hooks.tsx`: start a lab folder, mount the attack box
panel) are in a development frontend only, the one a debug build loads, or a build made with
`NEXT_PUBLIC_WDIO=1`. The specs that use them (`launch-ui`, `attack-box`, `attack-shell`,
`stop-with-box`, `park-resume`, `reimport-running`, `attack-on-stopped`, `fail-badlab`) fail on a
release app built without it.

Docker is called without a shell (`support/app.mjs`'s `docker`), so its `{{.Names}}` templates and
`^` filters reach it as written on Windows too.
