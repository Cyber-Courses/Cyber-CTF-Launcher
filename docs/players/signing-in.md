# Signing in

The labs of the catalogue need a Cyber CTF account; a lab imported from a folder runs without
one. You sign in in your own browser, on cyberauth.co: the launcher never asks for your password.

## Sign in

1. Select **Sign in** (in the first-run setup, or in **Settings > Account**). Your browser opens on
   cyberauth.co.
2. Sign in there, or confirm the account you're already signed in with.
3. The browser tab says you can close it, and the launcher shows your name in the sidebar and in
   **Settings > Account**.

You stay signed in across restarts. **Settings > Account > Sign out** ends the session on this
computer.

## When the browser doesn't help

While it waits, the launcher offers two other ways under the spinner:

- **Copy the sign-in link**: when no browser window opens, paste the link into a browser on this
  computer.
- **Use a code instead**: when this computer's browser can't be used (a remote desktop, a kiosk),
  the launcher shows a short code and a cyberauth.co page. Enter the code there from any device
  (your phone, another computer); the launcher signs in once you approve it. The code is good for
  up to 30 minutes.

**Cancel** always ends the wait.

## What the messages mean

| Message or problem                         | What to do                                                                                              |
| ------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| The sign-in was cancelled after 5 minutes  | Select **Sign in** again; finish in the browser within 5 minutes, or use a code.                        |
| You declined in the browser                | Select **Sign in** again and accept.                                                                    |
| Cyber CTF can't be reached                 | Check the connection; behind a proxy, see [troubleshooting](troubleshooting.md#signing-in).             |
| Signing in with a code isn't available yet | Use **Copy the sign-in link** instead.                                                                  |
| Sign out failed: the keychain refused      | Your system's keychain is locked or its prompt was dismissed: unlock it and sign out again.             |
| Signed out at every start (Linux)          | The session is kept in the Secret Service (GNOME Keyring, KWallet): make sure one runs and is unlocked. |

Your session is kept in your system's keychain (macOS Keychain, Windows Credential Manager, the
Secret Service on Linux), never in a file. How the sign-in works underneath, for those who change
it: [how sign-in works](../contributing/sign-in.md).
