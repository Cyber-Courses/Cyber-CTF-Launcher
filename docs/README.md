# Documentation

The launcher's documentation, by what you came to do. The repository's [README](../README.md) is
the way in: what the launcher is, how to install it, and what it needs.

## Use the launcher

| Page                                  | Kind     | What it covers                                                                        |
| ------------------------------------- | -------- | ------------------------------------------------------------------------------------- |
| [Getting started](getting-started.md) | tutorial | from a fresh install to a running lab and its attack box, on this machine with Docker |
| [Troubleshooting](troubleshooting.md) | how-to   | sign-in, Docker, VM labs, servers and clouds, tool installs, updates                  |
| [Signing in](signing-in.md)           | how-to   | the browser sign-in, the copy-the-link and code fallbacks, and how it works           |
| [Getting help](../SUPPORT.md)         | how-to   | where to ask, and what to send with a bug report                                      |

## Deploy and administer it

| Page                                              | Kind      | What it covers                                                                                                                        |
| ------------------------------------------------- | --------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| [Administrator guide](admin-guide.md)             | how-to    | silent install, updates, network endpoints and ports, proxy and CAs, offline use, files and data, machine changes, privacy, uninstall |
| [Policy](reference/policy.md)                     | reference | the machine-wide settings an administrator enforces                                                                                   |
| [Configuration](reference/configuration.md)       | reference | environment variables, command line, ports                                                                                            |
| [Versioning and support](reference/versioning.md) | reference | version numbers, which versions get fixes, supported platforms                                                                        |
| [Security policy](../SECURITY.md)                 | reference | reporting a vulnerability, response times, scope                                                                                      |

## Understand it

| Page                                        | Kind        | What it covers                                                                                                                  |
| ------------------------------------------- | ----------- | ------------------------------------------------------------------------------------------------------------------------------- |
| [Running labs](running-labs.md)             | explanation | where and how a lab runs: places and forms, the files that say where it is, the attack box, imported labs, accounts and servers |
| [Training apps](reference/training-apps.md) | reference   | deliberately vulnerable apps a lab can run (DVWA, Juice Shop): their pinned images, ready to paste                              |

## Develop it

| Page                                                    | What it covers                                                                  |
| ------------------------------------------------------- | ------------------------------------------------------------------------------- |
| [Contributing](../CONTRIBUTING.md)                      | set-up, the gates, the tests, commits and pull requests                         |
| [Architecture](architecture.md)                         | where labs run and what runs them, the code's layout, what is done              |
| [Testing](../tests/README.md)                           | the test tiers, what each needs, where a new test goes, and the conventions     |
| [The release check](testing/release-check.md)           | the gate before a release                                                       |
| [Desktop end-to-end](testing/desktop-e2e.md)            | the desktop end-to-end specs                                                    |
| [Integration tests](testing/integration.md)             | the integration tests and what each scenario guards against                     |
| [Release process](release-process.md)                   | making a release: version, tag, what the workflow signs and checks, its secrets |
| [Design system](design-system.md)                       | the launcher's design system: tokens, type, surfaces, components, patterns      |
| [Scripts](../scripts/README.md)                         | the repository's runner scripts                                                 |
| [Test servers](../infra/test-servers/README.md)         | throwaway Proxmox and ESXi hosts for the integration tests                      |
| [Microsoft Store listing](../packaging/store/README.md) | the Store listing that ships with each release                                  |
