# The command line: `cyberctf`

`cyberctf` drives the launcher without its window: the labs installed on this machine, their
attack boxes, the guided machine setup, the saved servers and cloud accounts, the account and the
storage. It is a program of its own, installed beside the app, and it runs the same code as the
app: every command calls the functions the app's own commands call, so what works here works in
the app, and the other way round.

It needs no window system. It runs on a server, a fresh virtual machine or a CI runner with no
display, with the app closed, which is its main use; it also runs beside an open app (below).

## Commands

| Command                                                                                                                    | What it does                                                                                                                                                                                         |
| -------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `cyberctf labs list`                                                                                                       | The labs installed here: how each runs, whether it runs, whether an operation acts on it.                                                                                                            |
| `cyberctf labs show <lab> [--show-secrets]`                                                                                | What the lab page's details card shows: where it runs, its machines, ports and addresses, what it exposes, the attack box's sign-in. Passwords and secret values are masked unless `--show-secrets`. |
| `cyberctf labs status <lab>`                                                                                               | Whether it runs, where, its machines' states, and its exploitability check when it runs.                                                                                                             |
| `cyberctf labs install <folder>`                                                                                           | Imports the lab in a folder (an unpublished lab, with its `isoloom.yml`) and starts it.                                                                                                              |
| `cyberctf labs start <lab> [--provider p] [--host h] [--offline] [--default-ports] [--per-machine-vm] [--docker-engine e]` | Starts a lab of the catalogue (signed in), as the launch picker does.                                                                                                                                |
| `cyberctf labs stop <lab>`                                                                                                 | Stops the lab wherever it runs and removes what it ran. Asks first.                                                                                                                                  |
| `cyberctf labs park <lab> [--shutdown]`                                                                                    | Pauses the lab (state saved), or shuts its machines down, keeping them.                                                                                                                              |
| `cyberctf labs resume <lab>`                                                                                               | Brings a parked lab back.                                                                                                                                                                            |
| `cyberctf labs remove <lab>`                                                                                               | Removes the lab from this computer, with everything the app keeps for it. Asks first.                                                                                                                |
| `cyberctf labs logs <lab> [--follow]`                                                                                      | The log of the lab's last operation in a deploy worker, followed while it runs.                                                                                                                      |
| `cyberctf attack-box start <lab> [--network n] [--image i] [--desktop]`                                                    | Adds the attack box to the running lab.                                                                                                                                                              |
| `cyberctf attack-box stop <lab>`                                                                                           | Takes it out.                                                                                                                                                                                        |
| `cyberctf attack-box status <lab> [--show-secrets]`                                                                        | Its state, its address and how to sign in to it.                                                                                                                                                     |
| `cyberctf machine check`                                                                                                   | What this machine has installed and can run (the Machine page).                                                                                                                                      |
| `cyberctf machine setup [--for docker,vm] [--provider p,…] [--engine e] [--dry-run]`                                       | The guided setup (below).                                                                                                                                                                            |
| `cyberctf machine tools list`                                                                                              | The tools Cyber CTF installed.                                                                                                                                                                       |
| `cyberctf machine tools install <tool>`                                                                                    | Installs a tool (`docker`, `vagrant`, `virtualbox`, `libvirt`, `qemu`, `terraform`, `awscli`, …), or a Vagrant plugin as `plugin:<name>`. Asks first.                                                |
| `cyberctf machine tools remove <tool>`                                                                                     | Removes a tool Cyber CTF installed. Asks first.                                                                                                                                                      |
| `cyberctf servers list \| show <id> \| test <id> \| remove <id> \| set-default <id> \| set-default --none \| public-key`   | The saved servers and cloud accounts. Adding one stays in the app's form.                                                                                                                            |
| `cyberctf cloud status`                                                                                                    | What each cloud CLI is signed in as.                                                                                                                                                                 |
| `cyberctf cloud signin aws\|azure\|gcp [--profile p]`                                                                      | Signs a cloud CLI in, in the browser.                                                                                                                                                                |
| `cyberctf account status \| signin [--browser] \| signout`                                                                 | The Cyber CTF account. `signin` gives a code to enter on any device; `--browser`, a link.                                                                                                            |
| `cyberctf storage show \| clean`                                                                                           | What Cyber CTF stored here; `clean` removes what no lab needs now. Asks first.                                                                                                                       |
| `cyberctf doctor`                                                                                                          | One report of what the app's health screens show: the machine, the account, the servers, the labs.                                                                                                   |
| `cyberctf version`                                                                                                         | This launcher's version, with the Isoloom and Isothrum it bundles.                                                                                                                                   |
| `cyberctf completions <shell>`                                                                                             | The shell completion script (below).                                                                                                                                                                 |

A lab command acts on the lab in the form its installed spec declares (containers, VMs or cloud
services); `--runtime docker|vm|cloud` says it when the spec can't.

Ctrl+C stops following an operation: one that runs in a deploy worker (a start, a resume, a
park) goes on there; one that runs in the command line itself (a stop, the attack box) ends.

Global options: `--json`, `--yes` (`-y`), `--data-dir <dir>` (or `CYBERCTF_DATA_DIR`: a data
folder other than the app's, for a sandbox or a test), `--verbose` (`-v`: the launcher's log on
stderr).

## Output

Text for a person by default. With `--json`, one JSON object a line on stdout:

```json
{"v":1,"type":"event","command":"labs.start","event":{"kind":"log","operation":"op-…","labId":"web","line":"==> Starting"}}
{"v":1,"type":"result","command":"labs.start","data":{"operation":"op-…","lab":"web","op":"launch"}}
{"v":1,"type":"error","command":"labs.start","exit":3,"error":{"code":"alreadyInProgress","message":"…","operation":null}}
```

- `v`: the shape's version, 1. Within a version a field may be added, never renamed, retyped or
  removed.
- `type`: `event` (as the command goes), then one `result` or one `error`, always last.
- `command`: the command's words joined by dots (`labs.start`, `machine.tools.install`).
- `event`: a lab operation's run events, as the app's windows get them (`started`, `log`,
  `ended`); a setup step's (`started`, `line`, `ended` with its `outcome` and `note`), each with the
  setup's `operation`; a tool's output lines.
- `error`: the app's own error (`code`, `args` when it has values, the English `message`), and the
  operation's id when the command ran one: quote it when asking for help.

## Exit codes

| Code | Meaning                                                                                                                                                                                                           |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | Done.                                                                                                                                                                                                             |
| 1    | The operation failed.                                                                                                                                                                                             |
| 2    | Usage: an unknown command or option, a value refused as given (an id, an image), a command that changes or deletes things run without `--yes` off a terminal.                                                     |
| 3    | Not ready: something to put right first. The lab isn't installed, another operation runs on it, Docker isn't running, the account is signed out, nothing can elevate unattended, a restart or a new login is due. |

## Confirmations and elevation

Stopping or removing a lab, cleaning the storage, the setup and installing or removing a tool ask
first on a terminal. Off one (a script), they need `--yes`, and are refused with exit 2 without
it.

The setup and the tool installs elevate as the app does when a person is there (a terminal, with
or without `--yes`): pkexec's password prompt on Linux, the macOS administrator prompt, Windows'
UAC prompt. With `--yes` off a terminal nobody can answer a prompt, so none is shown: the process
must be an administrator already (root, an elevated Windows terminal) or, on Linux, sudo must run
without a password (`sudo -n`). That is checked before anything runs, and the command exits 3 when
it can't; it never waits on a hidden prompt.

## The guided setup

`cyberctf machine setup` readies this machine for container labs, VM labs or both (`--for`), on
the hypervisors named by `--provider`, in order of preference (the first the setup can set up is
the one), with the engine `--engine` names. It checks the machine as the Machine page does, plans
what the setup window would do, in its order, and with `--dry-run` prints that plan and stops.
Otherwise, once confirmed, it runs each step with the same functions the window's buttons call,
streaming each step's lines and its outcome, and checks the machine again after each one, since
an install changes what comes next.

- A step that fails stops the setup: exit 1, with the step's name, its output's last lines and
  the setup's id.
- A step that needs a restart or a new login (WSL 2, Hyper-V, the `docker` group) is never run:
  the setup stops before it, says so and exits 3. Run it again after the restart or the login.
- Run on a machine already set up, it finds nothing to do and exits 0.

## Beside the open app

`cyberctf` uses the app's data folder, its saved servers, its store and its deploy workers. The
workers' pidfiles, the labs' locks and the store's transactions keep the two from acting on the
same lab at once: an operation the app runs refuses one from the command line, and the other way
round (exit 3).

Known limit: until the launcher reads its operations from Isothrum's runner service, each process
tells its own operations' events to itself only. The open app doesn't show an operation the
command line started while it runs; it picks it up at its next start.

## Installing it

Every package of the app carries `cyberctf`, beside the app:

- **Linux**: `/usr/bin/cyberctf` with the `.deb` and the `.rpm`, on the PATH; inside the AppImage
  too (`./Cyber_CTF.AppImage` runs the app; extract it, or use a package, for the command line).
- **Windows**: `cyberctf.exe` next to `Cyber CTF.exe` in the install folder. To put that folder on
  the PATH, install with `/CLIPATH` (the setup program, for your account) or `CLIPATH=1` (the MSI,
  for the machine: `msiexec /i "Cyber CTF.msi" CLIPATH=1`); uninstalling takes it off again.
- **macOS**: inside the app bundle, `Cyber CTF.app/Contents/MacOS/cyberctf`. The Homebrew cask
  links it onto the PATH; with the `.dmg`, link it yourself:

  ```sh
  sudo ln -sf "/Applications/Cyber CTF.app/Contents/MacOS/cyberctf" /usr/local/bin/cyberctf
  ```

## Shell completions

`cyberctf completions <shell>` prints the completion script for bash, zsh, fish, PowerShell or
elvish:

```sh
cyberctf completions bash > ~/.local/share/bash-completion/completions/cyberctf
cyberctf completions zsh > "${fpath[1]}/_cyberctf"
cyberctf completions fish > ~/.config/fish/completions/cyberctf.fish
cyberctf completions powershell >> $PROFILE
```
