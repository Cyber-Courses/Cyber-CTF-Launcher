# Machine-wide policy

An administrator can turn some launcher features off for everyone on a computer. The launcher
reads the policy from the operating system's managed location when it starts, and never writes it.
A change takes effect the next time the launcher starts.

## Settings

Every setting is a boolean. When a setting is missing, the feature stays on.

| Setting                | When on                                                                                                                                                                      |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `DisableWebsiteAgent`  | The computer doesn't register with Cyber CTF, doesn't show as online, and takes no launch requests from the website. Labs can still be started from the app itself.          |
| `DisableUpdater`       | The in-app updater is off. The app doesn't check the release feed and doesn't offer updates. Deploy updates through your own software distribution.                          |
| `DisableToolInstalls`  | One-click installs and removals of tools are refused: Docker, hypervisors, Vagrant and its plugins, Terraform, and the cloud CLIs. The player installs nothing from the app. |
| `DisableCloudLaunches` | No lab starts on a cloud account (AWS, Azure, Google Cloud, Oracle Cloud and the others). Labs on this computer and on the player's own servers still start.                 |
| `RefusePrivilegedLabs` | No lab with a privileged machine starts (`privileged: true` in its `isoloom.yml`: a container with kernel-level access to the computer that runs it).                        |

Names match regardless of case. The app's Settings window, under About, lists what the policy turns
off. The app doesn't offer what the policy turns off: the lab's launch picker shows the places it
refuses with the setting that refuses them, and the install and remove buttons of tools are
disabled with a line saying why. An operation the policy refuses anyway (a launch from the website,
a request made another way) fails before it changes anything, and says which setting refused it.

## Where the policy lives

### Windows

Registry values of type `REG_DWORD` under:

```text
HKEY_LOCAL_MACHINE\SOFTWARE\Policies\CyberCTF\Launcher
```

`1` turns a setting on, `0` leaves it off. Deploy them with Group Policy Preferences, Intune (a
custom OMA-URI or a registry script), or any tool that writes HKLM. For example:

```bat
reg add "HKLM\SOFTWARE\Policies\CyberCTF\Launcher" /v DisableToolInstalls /t REG_DWORD /d 1 /f
```

### macOS

A managed preferences file for the `org.cyberctf.desktop` domain:

```text
/Library/Managed Preferences/org.cyberctf.desktop.plist
```

An MDM installs it from a configuration profile with a Custom Settings (`com.apple.ManagedClient.preferences`)
payload for that domain. Each setting is a `<true/>` or `<false/>`. The launcher reads the
computer-level file only; user-level managed preferences (`/Library/Managed Preferences/<user>/`)
are ignored.

```xml
<dict>
  <key>DisableUpdater</key>
  <true/>
  <key>RefusePrivilegedLabs</key>
  <true/>
</dict>
```

### Linux

A JSON object in:

```text
/etc/cyberctf/policy.json
```

```json
{
  "DisableWebsiteAgent": true,
  "DisableCloudLaunches": true
}
```

Make it readable by everyone and writable by root only (`0644`, owned by root). The launcher takes
JSON because the format holds the same flat name-to-boolean shape as the registry and the property
list, configuration management tools (Ansible, Puppet, Salt) write and check it easily (`jq`), and it
has no implicit typing that could turn a typo into another value.

## Mistakes in the policy

The launcher logs each problem (see the log file below) and errs on the side of the policy:

| Problem                                                                     | What the launcher does                         |
| --------------------------------------------------------------------------- | ---------------------------------------------- |
| No policy at the location                                                   | Nothing is managed.                            |
| A setting it doesn't know                                                   | Logs a warning and ignores it.                 |
| A known setting whose value isn't a boolean (a string, a `REG_SZ`, a `2`)   | Logs a warning and treats that setting as on.  |
| The file or key is there but can't be read (bad JSON or plist, permissions) | Logs a warning and treats every setting as on. |

A malformed policy never re-enables a feature it was meant to turn off.

The log file is `cyberctf.log` in the app's log folder:

- Windows: `%LOCALAPPDATA%\org.cyberctf.desktop\logs`
- macOS: `~/Library/Logs/org.cyberctf.desktop`
- Linux: `~/.local/share/org.cyberctf.desktop/logs`

At startup it records which settings are on (`managed by policy: …`) and each problem.

## Updates installed by a package manager

Apart from `DisableUpdater`, the in-app updater turns itself off when a package manager owns the
installed copy, so the two never fight over its files:

- a Homebrew cask (its install receipt in `Caskroom` lists the app);
- a Debian package (the app runs from `/usr`, not `/usr/local`, and is not an AppImage);
- the Microsoft Store (the app runs from `Program Files\WindowsApps`).

Settings, under About, then says where updates come from.
