# Getting started

This tutorial takes you from a fresh install to a running lab and its attack box, on your own
computer, with Docker. It takes about 30 minutes, most of it downloads. You need a Cyber CTF
account and an internet connection.

## 1. Install the launcher

Install it for your system as the [README](../README.md#install) says, then open **Cyber CTF**.

## 2. Sign in

The first run opens a short setup. After the welcome, **Sign in** opens your browser on
cyberauth.co. Sign in there (or confirm the account you're already signed in with); the browser
tab then says you can close it, and the launcher shows your name.

If no browser opens, use **Copy the sign-in link** and paste it into a
browser. On a machine whose browser you can't use, **Use a code instead** shows a code to enter on
cyberauth.co from any device. [How sign-in works](signing-in.md).

## 3. Set up this machine

The setup checks what this machine can run and installs what is missing, one step at a time. Each
install asks for your password or an administrator's approval through your system's own prompt.

1. **Package manager**: Homebrew on macOS, winget (App Installer) on Windows, apt on Linux. The
   launcher installs the other tools with it.
2. **Virtualization** (Windows only): turns on WSL 2, which Docker Desktop needs. Windows restarts
   once; the launcher opens again after you sign back in.
3. **Container engine**: pick a Docker-compatible engine (Docker Desktop, or another the step
   offers for your system) and install it, or keep the one you have. Start it once it is
   installed.
4. **Test container labs**: starts a tiny two-container lab, checks it works, then deletes it.
5. **Attack machine**: pick the toolset of the attack box, the machine you attack labs from. Its
   image is a large download; you can change it later in Settings > Labs.
6. **Virtual machines**, **Provisioning** and **Test VM labs**: optional. Only VM labs need them; skip them for now.

You can come back to any step later from the **Machine** page.

## 4. Start a lab

1. Open **Labs**. The catalogue lists every lab; filter by **Container** to see the ones Docker
   runs.
2. Open a lab and select **Start lab**. If the lab can run in more than one place, the launcher
   asks where: pick **This machine** and **Container**.
3. The lab page follows the deployment step by step. The first start downloads the lab and its
   images, so it takes longer than the next ones.

## 5. Use the attack box

When the lab is up, its page shows the network and its machines, with the attack box among them.
Select **Open attacker shell** for a terminal on it, inside the lab's network. The folder
`~/CyberCTF/shared` on your computer is shared with the attack box: put tools in, take results
out. Its **Credentials** row has its account, `player`, and a password made for that box: use them
wherever the box asks you to sign in (its screen, `su`, SSH). `sudo` asks for no password. A
desktop attack VM's Screen window has the same in its toolbar, under **Sign-in**.

Solve the lab's objectives; your progress is saved to your Cyber CTF account.

## 6. Stop it

On the lab's page, **Pause** or **Shut down** keep the lab's state for later; **Stop & remove**
removes its machines, and the next start begins from scratch. Closing the launcher doesn't stop
a lab.

## Next

- Run VM labs: finish the optional VM steps on the **Machine** page.
- Run labs on your own Proxmox or ESXi server (**Servers**) or in your cloud account (**Cloud**):
  [running labs](running-labs.md#accounts-and-servers) says what each needs. Cloud labs are billed
  to your account; they stop on their own after the account's auto-stop time.
- Something went wrong? See [troubleshooting](troubleshooting.md).
