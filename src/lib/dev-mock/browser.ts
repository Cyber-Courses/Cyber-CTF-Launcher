// The browser preview's answers (see index.ts and the switches in scenario.ts): the scenario's
// machine, the sign-in, the saved servers and the lab operations, then the sample data.
import { fail, send, sleep } from "@/lib/dev-mock/command";
import { eventHandlers } from "@/lib/dev-mock/events";
import { type Host, hostsOf, saveError, saveHosts } from "@/lib/dev-mock/hosts";
import { hostedQuery } from "@/lib/dev-mock/hosted";
import { labHandlers } from "@/lib/dev-mock/lab-ops";
import type { BrowserHandler, Context } from "@/lib/dev-mock/live";
import { machineHandlers } from "@/lib/dev-mock/machine";
import { answer } from "@/lib/dev-mock/sample";
import { terminalHandlers } from "@/lib/dev-mock/terminal";
import type { Args } from "@/lib/dev-mock/types";
import type { DeviceCode } from "@/lib/tauri";

/** The sign-in, which `auth=out` starts without. */
const accountHandlers: Record<string, BrowserHandler> = {
  auth_status: (args, { live }) => (live.signedIn ? answer("auth_status", args) : { loggedIn: false, name: null, email: null }),
  auth_login: async (args, { live }) => {
    // The link the "Copy the sign-in link" fallback offers.
    send(args, "https://www.cyberauth.co/api/auth/oauth2/authorize?client_id=dev-mock", "events");
    await sleep(400);
    live.signedIn = true;
    return answer("auth_login", args);
  },
  auth_login_device: async (args, { live }) => {
    // The code the "Use a code instead" fallback shows, approved a moment later.
    const code: DeviceCode = { userCode: "WDJB-MJHT", verificationUri: "https://www.cyberauth.co/device", verificationUriComplete: null, expiresIn: 900 };
    send(args, code, "events");
    await sleep(1500);
    live.signedIn = true;
    return answer("auth_login_device", args);
  },
  agent_info: (args, { sc }) => ({ ...(answer("agent_info", args) as object), arch: sc.arch }),
  api_query: hostedQuery,
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
