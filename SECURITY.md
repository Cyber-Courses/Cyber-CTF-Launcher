# Security policy

## Supported versions

Security fixes go into the latest release only. Each fix ships as a new patch (or minor) release,
and the launcher offers it on its next update check; installs from Homebrew, apt and the
Microsoft Store get it through those channels.

| Version                               | Security fixes                           |
| ------------------------------------- | ---------------------------------------- |
| The latest release (see [Releases][]) | yes                                      |
| Anything older                        | no: update to the latest release instead |

[docs/reference/versioning.md](docs/reference/versioning.md) has the versioning and support policy
in full.

[Releases]: https://github.com/Cyber-Courses/Cyber-CTF-Launcher/releases

## Reporting a vulnerability

Please don't open a public issue. Report it privately through GitHub:
**[Report a vulnerability](https://github.com/Cyber-Courses/Cyber-CTF-Launcher/security/advisories/new)**
(the repository's _Security_ tab, then _Report a vulnerability_). If you can't use GitHub, email
hello@cyberctf.org (the contact in [cyberctf.org's security.txt][security.txt]) and say it is a
security report about the launcher.

Include what you can of:

- the launcher version (Settings > About) and the OS;
- what an attacker can do, and from where (another local user, a web page, the network, a lab);
- steps to reproduce, or a proof of concept;
- whether it is already public.

[security.txt]: https://www.cyberctf.org/.well-known/security.txt

## What happens next

| Step                                        | Within                              |
| ------------------------------------------- | ----------------------------------- |
| We acknowledge the report                   | 3 business days                     |
| We confirm it (or explain why not) and rate | 10 business days                    |
| A fix is released, for a critical or high   | 30 days after we confirm it         |
| A fix is released, for a medium or low      | the next planned release, ≤ 90 days |

We keep you informed until it is fixed, publish a GitHub security advisory with the release that
fixes it (crediting you, unless you'd rather not be), and request a CVE where one applies. Please
give us until that release before disclosing it.

## Scope

In scope: this repository: the desktop launcher, its installers and its release pipeline.

Out of scope, report them elsewhere:

- The Cyber CTF website, accounts and API (cyberctf.org, cyberauth.co, cyberbackend.com): use the
  contact in [cyberctf.org's security.txt][security.txt].
- The labs themselves. Labs are deliberately vulnerable training targets; a flaw inside a lab is
  the exercise. A way for a lab to reach beyond its own machines and networks into the player's
  computer, accounts or servers **is** in scope.
- Third-party tools the launcher drives (Docker, Vagrant, Terraform, the hypervisors, the cloud
  CLIs): report to their maintainers. Tell us too if the launcher's use of them makes it worse.

## How the launcher protects what it handles

For a security review, [docs/admin-guide.md](docs/admin-guide.md) lists the network endpoints, the
files and keychain items, and the changes the launcher makes to a machine;
[docs/architecture.md](docs/architecture.md) describes the design (a webview that only calls typed
Rust commands, tools run without a shell, tokens kept in the OS keychain). Releases after 1.8.9
are signed (Authenticode on Windows, Developer ID and notarization on macOS, minisign for in-app
updates) and come with SHA256SUMS, SBOMs and build provenance attestations
([docs/release-process.md](docs/release-process.md)). Up to 1.8.9, the macOS app was signed and
notarized but the Windows installers carried no Authenticode signature.
