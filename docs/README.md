# Documentation

The launcher's documentation, by who reads it: one folder per reader, each a section below
(`players/`, `admins/`, `lab-authors/`, `contributing/`). The repository's [README](../README.md)
is the way in: what the launcher is, how to install it, and what it needs.

## Use the launcher (`players/`)

| Page                                          | Kind        | What it covers                                                                                                                                                   |
| --------------------------------------------- | ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [Getting started](players/getting-started.md) | tutorial    | from a fresh install to a running lab and its attack box, on this machine with Docker                                                                            |
| [Troubleshooting](players/troubleshooting.md) | how-to      | sign-in, Docker, VM labs, servers and clouds, tool installs, updates                                                                                             |
| [Signing in](players/signing-in.md)           | how-to      | signing in in your browser, the link and code fallbacks, what the messages mean                                                                                  |
| [Getting help](../SUPPORT.md)                 | how-to      | where to ask, and what to send with a bug report                                                                                                                 |
| [The command line](players/cli.md)            | reference   | `cyberctf`: every command, its JSON lines and exit codes, the guided setup unattended                                                                            |
| [Running labs](players/running-labs.md)       | explanation | where a lab runs, the attack box (its account, password, shared folder, desktop), a machine's screen, importing and removing a lab, what servers and clouds need |

## Deploy and administer it (`admins/`)

| Page                                           | Kind      | What it covers                                                                                                                        |
| ---------------------------------------------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| [Administrator guide](admins/admin-guide.md)   | how-to    | silent install, updates, network endpoints and ports, proxy and CAs, offline use, files and data, machine changes, privacy, uninstall |
| [Policy](admins/policy.md)                     | reference | the machine-wide settings an administrator enforces                                                                                   |
| [Configuration](admins/configuration.md)       | reference | environment variables, command line, ports                                                                                            |
| [Versioning and support](admins/versioning.md) | reference | version numbers, which versions get fixes, supported platforms                                                                        |
| [Security policy](../SECURITY.md)              | reference | reporting a vulnerability, response times, scope                                                                                      |

## Write labs (`lab-authors/`)

| Page                                          | Kind      | What it covers                                                                                     |
| --------------------------------------------- | --------- | -------------------------------------------------------------------------------------------------- |
| [A lab's folder](lab-authors/lab-folder.md)   | reference | what the launcher reads from an imported lab's folder: `isoloom.yml`, and `.ctf/` for its page     |
| [Training apps](lab-authors/training-apps.md) | reference | deliberately vulnerable apps a lab can run (DVWA, Juice Shop): their pinned images, ready to paste |

## Develop it (`contributing/`)

| Page                                                       | What it covers                                                                                                              |
| ---------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| [Contributing](../CONTRIBUTING.md)                         | set-up, the gates, the tests, commits and pull requests                                                                     |
| [Architecture](contributing/architecture.md)               | where labs run and what runs them, the code's layout, what is done                                                          |
| [How labs run](contributing/how-labs-run.md)               | Isoloom, the runner and the launcher; the files that say where a lab is; the attack box in a lab; screens; removal; servers |
| [How sign-in works](contributing/sign-in.md)               | the OAuth flow, the tokens, and the client cyber-auth must have                                                             |
| [Testing](../tests/README.md)                              | the test tiers, what each needs, where a new test goes, and the conventions                                                 |
| [The release check](contributing/testing/release-check.md) | the gate before a release                                                                                                   |
| [Desktop end-to-end](contributing/testing/desktop-e2e.md)  | the desktop end-to-end specs                                                                                                |
| [Integration tests](contributing/testing/integration.md)   | the integration tests and what each scenario guards against                                                                 |
| [Release process](contributing/release-process.md)         | making a release: version, tag, what the workflow signs and checks, its secrets                                             |
| [Design system](contributing/design-system.md)             | the launcher's design system: tokens, type, surfaces, components, patterns                                                  |
| [Scripts](../scripts/README.md)                            | the repository's runner scripts                                                                                             |
| [Microsoft Store listing](../packaging/store/README.md)    | the Store listing that ships with each release                                                                              |
