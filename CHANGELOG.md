# Changelog

All notable changes to Cyber CTF Desktop are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

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
