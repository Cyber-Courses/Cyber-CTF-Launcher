# Running labs

Where a lab can run, what the attack box is, how to see a machine's screen, how to import your own
lab, how to remove one, and what a server or a cloud account needs. To start your first lab, see
[getting started](getting-started.md); how all this works underneath is in
[how labs run](../contributing/how-labs-run.md).

## Where a lab runs

A lab runs as containers or as virtual machines (VMs), here or elsewhere:

| Place                        | Container labs                | VM labs                        |
| ---------------------------- | ----------------------------- | ------------------------------ |
| **This machine**             | on Docker (or another engine) | one VM per machine (Vagrant)   |
| **A Proxmox or ESXi server** | no                            | one VM per machine             |
| **A cloud account**          | on one cloud instance         | one cloud instance per machine |

A container lab can also run as one VM per machine, here or in a cloud account, when it offers
that. When a lab can run in more than one place, **Start lab** asks where. Cloud labs are billed
to your account and stop on their own after the account's auto-stop time.

**Pause** and **Shut down** keep a lab for later, and **Resume** brings it back as it was. Pause
holds the lab where it is, for a resume in seconds: a VM lab's machines save their state, a
container lab's containers are frozen. Shut down powers the machines off (stops the containers),
keeping them: Resume boots them again. **Stop & remove** removes the lab's machines, and the next
start begins from scratch. Closing the launcher doesn't stop a lab.

## The attack box

The attack box is the machine you attack the lab from: Kali Linux with its tools, one more
machine on the lab's network. The lab page adds it, shows it among the lab's machines, and opens a
terminal on it with **Open attacker shell**.

| Lab          | The attack box                                                                                                            |
| ------------ | ------------------------------------------------------------------------------------------------------------------------- |
| Containers   | a container from the attack image picked in **Settings > Labs**                                                           |
| VMs          | a Kali VM (40 GB) that installs Kali's toolset on its first start (about 16 minutes); with a desktop when Settings say so |
| Arm computer | native: Kali's arm64 image beside containers; among VMs, a Debian arm64 VM turned into Kali on its first start            |

When a lab names a seat for the player (an access machine), the attack box takes that seat: its
name and addresses. Taking the box out puts the lab's own machine back.

**Your account in the box** is `player`: its shell opens as it, in its home, and `sudo` asks no
password. It also has a password, made for that box when it is added: use it wherever the box
asks you to sign in (its screen, `su`, SSH). The lab page's **Credentials** row shows it (masked
until **Show**, copied with a click), and a VM's Screen window has it in its toolbar under
**Sign-in**. A new box gets a new password; a box added by an earlier version has none.

**The shared folder**: `~/CyberCTF/shared` on your computer is shared with the box. Put tools in,
take results out.

**A desktop** (VM boxes only): turn on **Settings > Attack box > Attack VM desktop** to get Kali's
XFCE desktop, signed in as `player`, on the box's Screen. It needs about 1 GB more to download and
3 GB more on disk, and the VM gets 4 GB of memory and 2 CPUs. The desktop never locks you out: the
screen never blanks or locks; should a sign-in prompt still show (after a logout), the box's
password takes it. A container box has no desktop.

## A machine's screen

A VM with a graphical screen has a **Screen** button on the lab page (under Screens, and on the
attack box's card): it opens the machine's screen in a window of its own. Where a machine has no
screen, the page says why instead.

- Most VMs (libvirt, QEMU, VMware, Proxmox) show their screen right in the window.
- VirtualBox VMs and Windows VMs in a cloud open in your Remote Desktop client: Remote Desktop
  Connection on Windows, Windows App (or Microsoft Remote Desktop) on a Mac, FreeRDP or Remmina on
  Linux. The window says what to install when you have none. Use **Copy password** for the
  machine's password when the client asks.

Closing the window ends the session.

## Importing a lab from a folder

**Labs > Import a lab** runs a lab that isn't in the catalogue: pick a folder with an
`isoloom.yml`. The launcher copies the folder and starts the lab as containers on this machine,
without your Cyber CTF account (no sign-in, no flags, no progress saved). It is listed under
**Imported from a folder** on the Labs screen and under **Running** in the sidebar.

Its page shows the title, description and objectives the lab's own folder gives (how to write
them: [a lab's folder](../lab-authors/lab-folder.md)); the objectives are for reading only, since
only Cyber CTF checks answers. To pick up changes you made in the folder, stop the lab and import
the folder again.

## Removing a lab from this computer

A stopped lab's page has **On this computer > Remove lab**: an imported lab always, a catalogue
lab once it has been started here. It asks first, then deletes the launcher's copy of the lab and
everything the launcher keeps for it, including its attack box's password. The folder an imported
lab came from is never touched, and a catalogue lab's progress stays in your Cyber CTF account.

Remove lab is disabled, saying "Stop the lab first", while anything of the lab is up or an
operation runs on it. If something can't be deleted (a file held open), the launcher lists it with
why. The images and boxes the lab downloaded stay for the next lab:
**Settings > Maintenance > Cleanup** removes them once no lab uses them.

## Servers and cloud accounts

Add a server on the **Servers** page and a cloud account on the **Cloud** page. **Test** checks it
without running anything and says what is missing; the **VM test** of a server runs a throwaway
one-machine lab there and removes it.

- A cloud account signs in with keys stored in the launcher, or with its own command line's
  sign-in (`aws login`, `az login`, Google Cloud's application-default credentials, OCI's
  `~/.oci/config`). When that sign-in has run out, the launcher says how to sign in again before
  it changes anything.
- A cloud lab's firewall opens only to this computer's public address.

### What a server needs

- **Proxmox**: an API token (or root with its password); `snippets` and `iso` content on the
  `local` storage; a bridge with DHCP for the lab VMs; KVM (nested virtualization when Proxmox is
  itself a VM). When the lab bridge is internal to the node, the launcher reaches the lab VMs
  through the node, with its key let in there.
- **ESXi**: SSH on; promiscuous mode and forged transmits on the lab's port group; and VMware's
  OVF Tool on this computer (below).

### The OVF Tool for ESXi

Labs reach ESXi through VMware's OVF Tool (`ovftool`). The ESXi setup's tools step offers three
ways to get it:

- **Get VMware Fusion** (macOS) or **Get VMware Workstation** (Windows and Linux on x64): both are
  free from Broadcom and include the OVF Tool, where the launcher finds it.
- **Get the standalone OVF Tool**: Broadcom's download, an archive to unpack.
- **Use a downloaded OVF Tool…**: pick the unpacked folder (or the folder inside it that holds
  `ovftool`). The launcher copies it and leaves your download where it is. On macOS it checks the
  copy is signed by VMware before letting it run (the standalone tool isn't notarized).

The standalone macOS tool is built for Intel Macs: on Apple silicon it runs under Rosetta, and the
tools step says so when Rosetta isn't installed (`softwareupdate --install-rosetta`). The machine
check shows the version it found and where. When tool installs are turned off by your
administrator, these choices are too.
