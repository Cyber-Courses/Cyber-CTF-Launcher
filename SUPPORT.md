# Getting help

| You want to…                                              | Go to                                                                                  |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| Install the launcher or fix a common problem              | the [README](README.md) and [troubleshooting](docs/players/troubleshooting.md)         |
| Deploy it in a company (network, files, silent install)   | the [administrator guide](docs/admins/admin-guide.md)                                  |
| Report a bug or ask for a feature                         | [GitHub issues](https://github.com/Cyber-Courses/Cyber-CTF-Launcher/issues/new/choose) |
| Ask about your account, a subscription or a lab's content | [cyberctf.com/contact](https://www.cyberctf.com/contact)                               |
| Report a security problem                                 | [SECURITY.md](SECURITY.md), privately: not in an issue                                 |

## What to send with a bug report

The launcher gathers it for you, in **Settings > About**:

- **Diagnostics > Save diagnostics**: one file (`cyberctf-diagnostics-….tar.gz`) with the
  launcher's version, the OS, the machine check, the installed labs, the last lab operations and
  the end of the logs, with tokens, passwords, keys, flags and e-mail addresses removed and your
  home folder written `~`. Attach it to the issue. **Send to support** saves it the same way and
  opens a mail to support@cyberctf.com: attach the file there (a mail link can't attach it for you).
- **Diagnostics > Copy**: the launcher's version, the OS and the machine's setup status. Paste it
  into the issue.
- **Log files > Open folder**: `cyberctf.log`, and a `deploy-<lab>.log` per lab. Attach the ones
  from when it went wrong. Read them first: they name your labs, servers and paths.

Also say what you did, what you expected and what happened instead, and where the lab ran (this
machine, a server, a cloud account).

## What we support

The latest release, on the platforms in the [README](README.md#system-requirements). Older
versions get no fixes: update first ([versioning and support policy](docs/admins/versioning.md)).
Issues are answered on a best-effort basis; there is no guaranteed response time outside security
reports.
