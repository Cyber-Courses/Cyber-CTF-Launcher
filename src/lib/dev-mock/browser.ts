// The browser preview's answers (see index.ts and the switches in scenario.ts): the scenario's
// machine, the sign-in, the saved servers and the lab operations, then the sample data.
import { fail, send, sleep } from "@/lib/dev-mock/command";
import { eventHandlers } from "@/lib/dev-mock/events";
import { type Host, hostsOf, saveError, saveHosts } from "@/lib/dev-mock/hosts";
import { catalogueQuery, runOptionsQuery } from "@/lib/dev-mock/catalogue";
import { labHandlers } from "@/lib/dev-mock/lab-ops";
import type { BrowserHandler, Context } from "@/lib/dev-mock/live";
import { machineHandlers } from "@/lib/dev-mock/machine";
import { answer, UNMANAGED } from "@/lib/dev-mock/sample";
import { isSignedIn, sessionExpired, setSignedIn } from "@/lib/dev-mock/session";
import { terminalHandlers } from "@/lib/dev-mock/terminal";
import type { Args } from "@/lib/dev-mock/types";
import type { DeviceCode, ManagedSettings } from "@/lib/tauri";
import type { CommandError } from "@/lib/tauri/generated";
import { PROXMOX_PLACEMENT } from "@/lib/dev-mock/proxmox-sample";

/** Every policy switch on (`managed`): an administrator manages this computer. */
const MANAGED: ManagedSettings = {
  policy: ["DisableWebsiteAgent", "DisableUpdater", "DisableToolInstalls", "DisableCloudLaunches", "RefusePrivilegedLabs"],
  updates: "policy",
};

/** Ends the sign-in waiting, if one is (as the core's `auth_login_cancel`, or a sign-in with a
 *  code starting over a browser one): it rejects as cancelled. */
let cancelSignIn: (() => void) | null = null;

/** Settles once the `login` hold lets the sign-in through, or rejects as cancelled: taken at the
 *  start of a sign-in, so a cancel during its first moments isn't lost. */
function signInFinished(live: Context["live"]): Promise<void> {
  cancelSignIn?.();
  const finished = new Promise<void>((resolve, reject) => {
    cancelSignIn = () => {
      cancelSignIn = null;
      reject(cancelled());
    };
    void live.holds.passed("login").then(() => {
      cancelSignIn = null;
      resolve();
    });
  });
  // Rejected before it is awaited (a cancel during the sleep): not an unhandled rejection.
  finished.catch(() => {});
  return finished;
}

/** The core's `signInCancelled`: an Error (for the promise) that reads as a command error (its code). */
const cancelled = (): Error & CommandError => Object.assign(new Error("Sign-in cancelled."), { code: "signInCancelled" as const });

/** The sign-in, which `auth=out` starts without; shared by the preview's tabs (session.ts). */
const accountHandlers: Record<string, BrowserHandler> = {
  auth_status: (args, { sc }) =>
    isSignedIn(sc) ? answer("auth_status", args) : { loggedIn: false, name: null, email: null, ...(sessionExpired(sc) && { sessionExpired: true }) },
  auth_login: async (args, { live }) => {
    // The link the "Copy the sign-in link" fallback offers.
    const finished = signInFinished(live);
    send(args, "https://www.cyberauth.co/api/auth/oauth2/authorize?client_id=dev-mock", "events");
    await sleep(400);
    await finished;
    setSignedIn(true);
    return answer("auth_login", args);
  },
  auth_login_cancel: () => {
    cancelSignIn?.();
    return null;
  },
  auth_login_device: async (args, { live }) => {
    // The code the "Use a code instead" fallback shows, approved a moment later.
    const code: DeviceCode = { userCode: "WDJB-MJHT", verificationUri: "https://www.cyberauth.co/device", verificationUriComplete: null, expiresIn: 900 };
    const finished = signInFinished(live);
    send(args, code, "events");
    await sleep(1500);
    await finished;
    setSignedIn(true);
    return answer("auth_login_device", args);
  },
  auth_logout: () => {
    setSignedIn(false);
    return null;
  },
  agent_info: (args, { sc }) => ({ ...(answer("agent_info", args) as object), arch: sc.arch }),
  managed_settings: (_args, { sc }) => (sc.managed ? MANAGED : UNMANAGED),
  api_query: catalogueQuery,
  lab_run_options: runOptionsQuery,
};

/** The saved servers and cloud accounts, kept across the preview's tabs. */
const serverHandlers: Record<string, BrowserHandler> = {
  server_list: (args, { sc }) => ({ ...(answer("server_list", args) as object), hosts: hostsOf(sc) }),
  server_save: (args, { sc }) => {
    const input = (args?.input ?? {}) as Host;
    const invalid = saveError(input);
    if (invalid) return fail(invalid);
    const saved = { ...input, id: input.id ?? `host-${String(Date.now())}`, password: undefined };
    saveHosts([...hostsOf(sc).filter((h) => h.id !== saved.id), saved]);
    return saved;
  },
  // A Proxmox server's placement, as a one-node home server lists it.
  server_proxmox_placement: () => PROXMOX_PLACEMENT,
  // The preview's node "makes" the launcher's token: saved as a token setup that keeps its key.
  server_save_ssh_key: (args, { sc }) => {
    const input = (args?.input ?? {}) as Host;
    const saved = { ...input, id: input.id ?? `host-${String(Date.now())}`, username: "root@pam!cyberctf-preview", password: undefined };
    saveHosts([...hostsOf(sc).filter((h) => h.id !== saved.id), saved]);
    return saved;
  },
};

/** The window and the link the app was opened with. */
const windowHandlers: Record<string, BrowserHandler> = {
  "plugin:window|close": () => {
    // The app closes its window; here, the tab (one the app opened, as its windows are).
    setTimeout(() => {
      window.close();
    }, 0);
    return null;
  },
  "plugin:deep-link|get_current": (_args, { sc }) => (sc.link ? [sc.link] : null),
  // The system dialog a destructive action asks in (`confirmDestructive`): the browser's own.
  "plugin:dialog|message": (args) => {
    const [ok, cancel] = (args?.buttons as { OkCancelCustom?: [string, string] } | undefined)?.OkCancelCustom ?? ["Ok", "Cancel"];
    return window.confirm([args?.title, args?.message].filter(Boolean).join("\n\n")) ? ok : cancel;
  },
};

// A Map, not an object lookup: a command named like an Object.prototype member is answered by
// the sample data.
const HANDLERS = new Map<string, BrowserHandler>(
  Object.entries({ ...eventHandlers, ...machineHandlers, ...accountHandlers, ...labHandlers, ...terminalHandlers, ...serverHandlers, ...windowHandlers }),
);

/** The browser preview's answer to `cmd`. */
export async function browserAnswer(cmd: string, args: Args, ctx: Context): Promise<unknown> {
  const handler = HANDLERS.get(cmd);
  return handler ? await handler(args, ctx) : answer(cmd, args);
}
