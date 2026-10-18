# Changelog

All notable changes to Cyber CTF Desktop are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[semantic versioning](https://semver.org/spec/v2.0.0.html).

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
