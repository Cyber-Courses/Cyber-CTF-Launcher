# What the launcher is, and what it is not

The launcher is one of three products that work together. Each has a boundary the others rely
on. This page states the launcher's.

## What it is

The CTF layer. It knows the player, their account and sign-in, the lab catalogue, a lab's page and
its flags, the attack box, the player's servers and cloud accounts, and the words the app shows.
Everything that runs is a thin wrapper over the other two products:

- [Isoloom](https://github.com/isoloom/isoloom)'s crates turn a lab's `isoloom.yml` into native
  files (Compose, Vagrant, Terraform), in `runtime/lab/`;
- [Isothrum](https://github.com/Isothrum/isothrum)'s runner service and client run those files,
  wherever the lab runs (`runtime/isothrum/`), and `isothrum-machine` says what this computer's
  hypervisors and tools can do.

## What it is not

- **Not a runtime.** The launcher doesn't run `docker`, `vagrant`, `terraform`, a hypervisor's
  command line (`VBoxManage`, `prlctl`, `vmrun`, `utmctl`, `virsh`, `qemu-*`) or a cloud's CLI
  itself. Starting, stopping and reading a deployment, and checking a hypervisor, are Isothrum's.
- **Not a second error mapper.** Isothrum's errors (`isothrum_runner::Error`, the service's
  `ErrorDetail`) become the launcher's coded errors in exactly one place,
  `runtime/isothrum/errors.rs`. Elsewhere the launcher passes them on, it doesn't read them.
- **Not where a generic feature goes.** When a lab needs something any Compose, Vagrant or
  Terraform project could use (a retry, an error kind, a way to reach a machine), it goes into
  Isothrum under a neutral name, and the launcher calls it.
- **Not a special case in Isoloom.** Isoloom's output has the same contract for every spec. When a
  lab's generated files need a value (an environment variable, a Terraform input), the lab
  declares it and the launcher supplies it; Isoloom isn't changed for one lab.

## Where it stands

The launcher isn't there yet: some of its code still runs tools and reads their output itself,
from before Isothrum could. A migration moves that code into Isothrum and `isothrum-machine`
step by step, and deletes it here. Until it ends, new code keeps to the rules above, and code
that moves is never moved back.

## Its boundary with the other products

| Product  | What it is                                                            | What the launcher may do with it                                                                                                                             |
| -------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Isoloom  | A converter: one spec into each target's native files                 | Read a lab's spec and generate its files, through `runtime/lab/` only                                                                                        |
| Isothrum | A general-purpose runtime for Compose, Vagrant and Terraform projects | Run and read deployments through its client and service, and hypervisors through `isothrum-machine`; convert its errors in `runtime/isothrum/errors.rs` only |
