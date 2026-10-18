// Hosted labs in the browser preview (Cyber CTF runs them): a session goes from requested to
// running on the next poll, with its public address (with `hold=hosted`, on the first poll after
// the test lets it).
import { argText } from "@/lib/dev-mock/command";
import { type BrowserHandler, type Live, slugOf } from "@/lib/dev-mock/live";
import { answer } from "@/lib/dev-mock/sample";

/** A hosted session requested for `labId`, ending in two hours. */
function requested(labId: string): NonNullable<Live["hosted"]> {
  return {
    id: `sess-${String(Date.now())}`,
    labId,
    target: "hosted",
    state: "REQUESTED",
    endpoints: [],
    message: null,
    expiresAt: new Date(Date.now() + 2 * 3600_000).toISOString(),
  };
}

/** A GraphQL operation (`api_query`): the hosted session's, else the sample data's. */
export const hostedQuery: BrowserHandler = (args, { live }) => {
  const op = argText(args?.operation);
  const vars = (args?.variables ?? {}) as Record<string, unknown>;
  if (op === "activeSession") return { myActiveLabSession: live.hosted };
  if (op === "launchHosted") {
    live.hosted = requested(argText(vars.labId));
    return { requestLabLaunch: live.hosted };
  }
  if (op === "labSession") {
    const h = live.hosted;
    if (!h || h.id !== vars.id) return { labSession: null };
    if (live.holds.isHeld("hosted")) return { labSession: h };
    // One poll later it runs, with its public address.
    live.hosted = { ...h, state: "RUNNING", endpoints: [{ port: 443, url: `https://${slugOf(h.labId)}-7f3a.labs.cyberctf.org` }] };
    return { labSession: live.hosted };
  }
  if (op === "stopSession") {
    live.hosted = null;
    return { stopLabSession: true };
  }
  return answer("api_query", args);
};
