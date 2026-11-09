# Configuration

What the launcher reads from its environment. Users set everything else in the app (Settings);
an administrator's machine-wide settings are in the [policy reference](policy.md).

## Environment variables

Read when the app starts. A variable set to an empty value counts as unset.

| Variable                                             | Default                                                                 | What it does                                                                                                                                                  |
| ---------------------------------------------------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `CYBERCTF_AGENT_NAME`                                | `<user>'s Mac`, `'s PC` or `'s Linux`                                   | the machine name the launcher registers with Cyber CTF (see [Privacy](../admin-guide.md#privacy-and-what-is-sent)); set it to keep the OS user name out of it |
| `CYBERCTF_AUTH_BASE`                                 | `https://www.cyberauth.co/api/auth`                                     | the Cyber Auth (sign-in) endpoint                                                                                                                             |
| `CYBERBACKEND_URL`                                   | `https://cyberbackend.com`                                              | the CyberBackend API, also the access token's audience                                                                                                        |
| `CYBERCTF_CLIENT_ID`                                 | `MORSNcedpYSLupYIwbaKyHIXdGgVhanC`                                      | the launcher's OAuth client in Cyber Auth (public, PKCE)                                                                                                      |
| `CYBERCTF_REDIRECT`                                  | `loopback`                                                              | `scheme` brings sign-in back as a `cyberctf://auth/callback` link instead of to `http://127.0.0.1:<port>/callback`; the OAuth client must list that URI       |
| `ISOLOOM_HYPERV_SWITCH`                              | Hyper-V's "Default Switch", else an external switch, else the first one | the Hyper-V virtual switch a VM lab's first network adapter uses (Windows)                                                                                    |
| `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` | the system proxy settings (macOS, Windows)                              | the proxy for the launcher's own connections ([Proxy](../admin-guide.md#proxy-and-custom-certificate-authorities))                                            |

`CYBERCTF_AUTH_BASE`, `CYBERBACKEND_URL`, `CYBERCTF_CLIENT_ID` and `CYBERCTF_REDIRECT` exist to run
the launcher against a test or local instance of Cyber CTF's services
([`src-tauri/src/platform/config.rs`](../../src-tauri/src/platform/config.rs)). They can also be
fixed at build time, by setting them in the environment of `pnpm tauri build`; a value set when
the app runs wins over the built-in one.

The tools the launcher runs (Docker, Vagrant, Terraform, the cloud CLIs) also read their own
variables from the environment the app was started with.

## Command line

The app's executable takes:

| Argument              | What it does                                                                        |
| --------------------- | ----------------------------------------------------------------------------------- |
| none                  | starts the app; when it already runs, brings its window forward instead             |
| a `cyberctf://` link  | opens the link in the running app (the browser passes the website's links this way) |
| `deploy --job <file>` | internal: the detached worker that runs one lab operation, started by the app       |

## Ports

The sign-in redirect listens on `127.0.0.1`, on the first free port of 47290, 47291 and 47292,
while a sign-in waits. Each must be registered on the OAuth client as
`http://127.0.0.1:<port>/callback` ([signing-in.md](../signing-in.md#the-oauth-client)).
