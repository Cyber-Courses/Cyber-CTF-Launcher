# Versioning and support policy

## Version numbers

The launcher follows [semantic versioning](https://semver.org/spec/v2.0.0.html), `MAJOR.MINOR.PATCH`,
for what its users and administrators rely on: how it is installed and removed, the
[policy](policy.md) and [configuration](configuration.md) it reads, the files and keychain entries
it keeps ([administrator guide](../admin-guide.md#files-and-data)), and the labs it can run.

| Part  | Goes up when                                                                                                                                                              |
| ----- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| MAJOR | something that worked stops working without action: a platform or OS version is dropped, a policy setting or variable is removed or changes meaning, data is not migrated |
| MINOR | something is added: a feature, a platform, a policy setting; deprecations are announced here                                                                              |
| PATCH | fixes only, security fixes included                                                                                                                                       |

The same number is used everywhere: the Git tag (`v1.8.9`), `package.json`,
`src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, the installers, the Microsoft Store package
(`1.8.9.0`) and the Homebrew cask and apt package. `CHANGELOG.md` lists every release's changes.

## Support

- **Only the latest release is supported.** Bug and security fixes ship as a new release; there
  are no fixes to older versions. The in-app updater, Homebrew, apt and the Store make updating a
  single step.
- A security fix is released as a patch on the latest version, within the times in
  [SECURITY.md](../../SECURITY.md).
- Something being removed is announced in the changelog's **Deprecated** section at least one
  minor release before it goes.

## Platforms

A release supports the platforms its installers are built and tested on (`release.yml`,
`ci.yml`):

| Platform | Versions                                                                                     | Architectures        |
| -------- | -------------------------------------------------------------------------------------------- | -------------------- |
| Windows  | Windows 10 version 2004 (build 19041) and later, Windows 11                                  | x64, Arm64           |
| macOS    | the releases Apple still updates (the app itself opens on 10.13 and later)                   | Apple silicon, Intel |
| Linux    | distributions with glibc 2.35 or later and WebKitGTK 4.1 (Ubuntu 22.04, Debian 12 and later) | x64, arm64           |

The tools labs need (Docker, Vagrant, the hypervisors) have their own platform support, which can
be narrower.
