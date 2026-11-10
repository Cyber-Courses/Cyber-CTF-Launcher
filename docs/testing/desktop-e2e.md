# Desktop end-to-end

`e2e/desktop/run.sh` builds the app with `--features wdio` (the embedded W3C WebDriver
server, never in a release build), serves the frontend a debug build loads (`tauri.conf.json`'s
`devUrl`, port 1437; a release build embeds `out/` and needs none), checks out the fixture lab
(the Supplier Portal API lab, under `infra/test-servers/.state/labs`), and runs the scenarios in an
order that leaves nothing behind:

| Scenario            | Proves                                                                                                                                                                                                                                                                                                                             |
| ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `smoke`             | the app boots and its shell renders                                                                                                                                                                                                                                                                                                |
| `fail-offline`      | with the backend unreachable (`CYBERBACKEND_URL` dead), the catalogue degrades and the app stays usable                                                                                                                                                                                                                            |
| `fail-badlab`       | a folder that is not a lab is refused without a crash or a phantom running lab                                                                                                                                                                                                                                                     |
| `fail-signedout`    | signed out (an empty `HOME`, where a debug build keeps its session), the app is usable and asks to sign in                                                                                                                                                                                                                         |
| `fail-docker`       | with Docker unreachable (a dead `DOCKER_HOST` and a `DOCKER_CONFIG` naming no current context, since the app follows the user's Docker context over an inherited `DOCKER_HOST`), the header says Docker is not running, the seed lab's start fails and nothing runs                                                                |
| `launch`            | the seeded local lab reaches "running on this machine" on the Overview                                                                                                                                                                                                                                                             |
| `restart`           | after the app is killed and reopened without the seed, the running-labs scan finds the lab again                                                                                                                                                                                                                                   |
| `stop`              | Stop in Machine > Running now stops and clears it                                                                                                                                                                                                                                                                                  |
| `launch-ui`         | the real import path (`__wdio_launch_local__`) reaches running                                                                                                                                                                                                                                                                     |
| `attack-box`        | the attack box attaches, gets an address, opens a shell and detaches                                                                                                                                                                                                                                                               |
| `stop-with-box`     | stopping a lab through `lab_stop` while its attack box is attached leaves no container of the lab behind, the box included (leaked in 1.5.3)                                                                                                                                                                                       |
| `launch-vm`         | a catalogue container lab started "in a VM" on UTM through the real "Where should it run?" dialog reaches running (its published port answers), stops from its page and leaves no VM in UTM; needs the app signed in and UTM with its Vagrant plugin (skipped otherwise)                                                           |
| `onboarding`        | the first-run wizard renders in the main window, advances, and Skip setup lands on the shell                                                                                                                                                                                                                                       |
| `setup-screens`     | the Servers and Cloud screens render with their setup entry points, the shell usable                                                                                                                                                                                                                                               |
| `attack-on-stopped` | the attack box panel on a lab that isn't running shows it isn't, never a box up                                                                                                                                                                                                                                                    |
| `park-resume`       | a running container lab parks (nothing of it runs) and resumes                                                                                                                                                                                                                                                                     |
| `reimport-running`  | importing a lab that is already running leaves exactly one copy and a usable app                                                                                                                                                                                                                                                   |
| `kill-mid-deploy`   | killed mid-deploy, the app reopens coherently                                                                                                                                                                                                                                                                                      |
| `vm-scan`           | on a Mac with UTM: a started VM named `<lab> · <machine>` is matched to the installed lab by the running-labs scan; `<lab>` is the lab's isoloom name plus `-<instance>` when it holds an instance number (the fixture VM is created and deleted by the runner; Docker is made unreachable so only the hypervisor probe can match) |

Each scenario writes `e2e/desktop/.state/logs/<name>.log`. Knobs: `APP_BIN`,
`TAURI_WEBDRIVER_PORT`, `DEV_URL_PORT`, `CYBERCTF_TEST_LOCAL_LAB`, `CASE_TIMEOUT`, `ONLY`,
`SKIP_BUILD`, `KEEP` (leave the lab running), `STRICT` (a skip fails the run). The scenarios are `e2e/desktop/specs/<name>.mjs` and share `e2e/desktop/support.mjs`; a new
one is a short `.mjs` there plus a line in `e2e/desktop/run.sh`.

On Linux the VM scan is skipped: start a libvirt domain named `<lab>_<machine>` (or a Hyper-V VM
`<lab>-<machine>`) yourself, `<lab>` being the installed lab's isoloom name plus `-<instance>` when
`labs/<id>/.cyberctf-instance` holds a number, and run `e2e/desktop/specs/vm-scan.mjs` with
`CYBERCTF_EXPECT_LAB=<lab id>`.
On Windows, `e2e/desktop/smoke.ps1` runs the smoke; the scenarios take `APP_BIN` there too.

The `__wdio_*` hooks (`src/components/wdio-hooks.tsx`: start a lab folder, mount the attack box
panel) are in a development frontend only, the one a debug build loads: a release build leaves
them out. A release app driven by these scenarios (`APP_BIN`) is built with `NEXT_PUBLIC_WDIO=1`
(`NEXT_PUBLIC_WDIO=1 pnpm tauri build --features wdio`), which keeps them.

Why a debug build needs the dev server: Tauri's `generate_context!` loads `devUrl` in a debug
build and the static export only in a release build, so a debug app started by hand shows a blank
window until `next dev` answers on port 1437. The runner handles it.

Why the scripts force the page visible: the frontend pauses its polls while the document is
hidden (`src/lib/visibility.ts`), and on macOS a window another one covers reports hidden, so a
driven app never learned that its lab came up. `support.mjs` overrides `document.visibilityState`
and re-applies it on every wait tick (a reload drops it).
