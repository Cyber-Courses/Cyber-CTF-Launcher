# Desktop end-to-end

The real app, driven through its embedded W3C WebDriver server (`tauri-plugin-wdio-webdriver`,
built in with `--features wdio`, never in a release build): no external driver, so the same specs
run on Linux, Windows and macOS. One entry point a system:

```sh
tests/desktop/run.sh --all                                   # Linux, macOS
tests/desktop/run.sh --specs "smoke settings" --knobs ~/.config/cyberctf-test/e2e.env
tests/desktop/run.sh --app path/to/cyberctf-desktop          # an app already built
tests/desktop/run.sh --package path/to/Cyber.CTF_1.8.9_amd64.deb   # .deb, .rpm or .AppImage, unpacked
```

```powershell
pwsh -File tests\desktop\run.ps1 -All                        # Windows, in the desktop session
pwsh -File tests\desktop\run.ps1 -Specs "smoke settings" -Knobs $HOME\.config\cyberctf-test\e2e.env
pwsh -File tests\desktop\run.ps1 -Package path\to\Cyber.CTF_1.8.9_x64_en-US.msi   # or the NSIS setup .exe
```

Without `--app` or `--package` the runner builds the app with `cargo build --features wdio` and
serves the frontend such a build loads (`next dev` on 3206, `tauri.conf.json`'s `devUrl`; an app
from `pnpm tauri build` embeds `out/`, a `cargo build --release` doesn't). A package must be a
`tauri build` with the hooks: `NEXT_PUBLIC_WDIO=1 pnpm tauri build --features wdio`, as
`native-e2e.yml` builds it
(the `NEXT_PUBLIC_WDIO=1` keeps the `__wdio_*` hooks a few specs use). Before the specs, the runner
starts the app once and stops with a clear message when no WebDriver server answers. It checks out
the fixture lab (the Invoice Portal API lab, under `infra/test-servers/.state/labs`, or
`tests\desktop\.state\labs` on Windows) unless `CYBERCTF_TEST_LOCAL_LAB` names one.

It is safe on a machine someone uses: the app keeps all of its own (data, settings, logs, webview
data, session, secrets) in a folder made for the run and removed after it, and every clean-up
removes only what the run made, by the records it wrote: never the user's labs, images, boxes or
apps ([tests/README.md](../../tests/README.md#a-desktop-runs-own-folder)).

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
   title-bar            ok        41s  tests/desktop/.state/logs/title-bar.log
   title-bar/controls   ok         3s  the main window's header ends with Minimize, Maximize and Close
   lab-form-vm              skip       0s  no hypervisor named: set CYBERCTF_TEST_VM_PROVIDER (...)
```

With `--results <file>` (`-Results` on Windows) the runner also appends a JSON record per spec and
per check there, and `--list` (`-List`) prints its cases ([tests/README.md](../../tests/README.md#results)).
Its exit status: 0 nothing failed, 1 a spec failed, 2 the runner itself couldn't run; `STRICT=1`
fails a skip. Logs: `tests/desktop/.state/logs/<spec>.log`.

## Knobs

From the environment or the `--knobs` file (`KEY=value` lines, read literally, never sourced,
as the test rig writes its settings files). [tests/README.md](../../tests/README.md#knobs) lists
each, with the specs that read it.

## Feature map

Every feature a player uses, the spec that drives it, what it needs, and what is left to the
integration tests (`docs/testing/integration.md`) or the unit tests. Cloud accounts are out of
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
| A local lab as containers in one VM (Docker-in-a-VM)                            | `lab-form-docker-vm`                                                                                | signed in, `CYBERCTF_TEST_VM_PROVIDER`                                     | `docker-vm-lab`                                                                          |
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
| The running-labs scan finding a VM lab                                          | `vm-scan`                                                                                           | a Mac with UTM (the runner makes the VM); elsewhere by hand (below)        |                                                                                          |

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
| `park-resume`       | a running container lab parks (nothing of it runs) and resumes                                                                                                                                                                               |
| `reimport-running`  | importing a lab that is already running leaves exactly one copy and a usable app                                                                                                                                                             |
| `attack-on-stopped` | the attack box panel on a lab that isn't running shows it isn't, never a box up                                                                                                                                                              |
| `lab-actions`       | a catalogue container lab: Pause and Resume; a container stopped behind its back shows as down and Reset lab starts it again; Stop & remove answered Cancel keeps it running, answered Remove leaves nothing                                 |
| `offline-start`     | a lab started online, then started with the backend cut (through the spec's relay), is offered an offline start, runs offline, and stops                                                                                                     |
| `lab-form-*`        | `lab-form` with `CYBERCTF_TEST_LAB_FORM`: a catalogue lab started in that form through the "Where should it run?" dialog runs (asked of Docker or the hypervisor), its address answers, and Stop & remove leaves nothing                     |
| `screens`           | a running VM lab's Screens panel; a VNC screen's window shows its framebuffer; an RDP one's window hands it to the system's client; a machine without a screen says why; with `CYBERCTF_TEST_BOX_DESKTOP=1`, the attack box's desktop Screen |
| `vm-scan`           | on a Mac with UTM: a started VM named `<lab> · <machine>` is matched to the installed lab by the running-labs scan (the fixture VM is made and deleted by the runner; Docker is made unreachable so only the hypervisor probe can match)     |
| `server-lab`        | Add server, the setup window filled from the settings file (tools installed from it when missing), Save and test passing, the VM test, the server listed and Online, a catalogue lab started, running and removed there, the server removed  |
| `cleanup`           | Settings > Maintenance: Clean up answered Cancel removes nothing; answered Remove, when every download it lists is the run's, says what it freed (otherwise the others are named and all left); a named tool the run's app installed removed |

The specs are `tests/desktop/specs/<name>.mjs` and share `tests/desktop/lib/support.mjs`. A case's
id is its spec's name, which its summary line and its log (`<id>.log`) carry; a spec run once per
form adds the form: `lab-form` runs as `lab-form-container`, `lab-form-docker-vm`, `lab-form-vm` and
`lab-form-per-machine`. A new spec is a short `.mjs` there (its checks through `results()`, its
skips through `skipAll`) plus a line in `run.sh` and `run.ps1` and a row above.

On Linux the VM scan is skipped: start a libvirt domain named `<lab>_<machine>` (or a Hyper-V VM
`<lab>-<machine>` on Windows) yourself, `<lab>` being the installed lab's isoloom name plus
`-<instance>` when `labs/<id>/.cyberctf-instance` holds a number, and run
`tests/desktop/specs/vm-scan.mjs` with `CYBERCTF_TEST_SCAN_LAB=<lab id>`.

## On the test rig

The rig (the workspace's `rig/`) runs the set on a fresh Linux or Windows clone, in the logged-in
desktop session (on Windows a scheduled task of that account; on a headless Linux,
`xvfb-run -a dbus-run-session --`). Its knobs file, outside every repository
(`~/.config/cyberctf-test/e2e.env`, mode 600):

```sh
CYBERCTF_TEST_VM_PROVIDER=libvirt            # hyperv or virtualbox on Windows
CYBERCTF_TEST_ACCOUNT_SESSION=/home/vagrant/.config/cyberctf-test/launcher-session.json
CYBERCTF_TEST_CATALOGUE_VM_LAB=<a catalogue VM lab's title>
CYBERCTF_TEST_SERVER_ENV=/home/vagrant/.config/cyberctf-test/rig-proxmox.env
```

Then, from the launcher's checkout in the clone:

```sh
xvfb-run -a dbus-run-session -- tests/desktop/run.sh --all --knobs ~/.config/cyberctf-test/e2e.env
```

```powershell
pwsh -File tests\desktop\run.ps1 -All -Knobs $HOME\.config\cyberctf-test\e2e.env
```

The rig reads the records `--results` writes ([tests/README.md](../../tests/README.md#results)):
a line per spec, and a line per check as `<spec>/<check>`; the summary on screen says the same.

## Why the runner does what it does

An app from `cargo build` needs the dev server: the frontend is embedded only with Tauri's
`custom-protocol` feature, which `pnpm tauri build` turns on and a plain `cargo build` doesn't,
`--release` or not. Without it the app loads `devUrl`, and started by hand shows a blank window
until `next dev` answers on port 3206.

The specs force the page visible: the frontend pauses its polls while the document is hidden
(`src/lib/visibility.ts`), and on macOS a window another one covers reports hidden, so a driven app
never learned that its lab came up. `support.mjs` overrides `document.visibilityState` and
re-applies it on every wait tick (a reload drops it).

The `__wdio_*` hooks (`src/components/wdio-hooks.tsx`: start a lab folder, mount the attack box
panel) are in a development frontend only, the one a debug build loads, or a build made with
`NEXT_PUBLIC_WDIO=1`. The specs that use them (`launch-ui`, `attack-box`, `attack-shell`,
`stop-with-box`, `park-resume`, `reimport-running`, `attack-on-stopped`, `fail-badlab`) fail on a
release app built without it.

Docker is called without a shell (`support.mjs`'s `docker`), so its `{{.Names}}` templates and
`^` filters reach it as written on Windows too.
