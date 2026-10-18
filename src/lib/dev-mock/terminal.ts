// A stand-in shell for the attack box in the browser preview: a prompt, an echo, and a few
// commands' answers (`id`, `whoami`, `hostname`, `ip -br a`); it ends on `exit`.
import { argText, fail, send } from "@/lib/dev-mock/command";
import type { BrowserHandler } from "@/lib/dev-mock/live";
import type { Args } from "@/lib/dev-mock/types";
import { openWindow } from "@/lib/dev-mock/windows";

const PROMPT = "\x1b[1;31mroot@attack-box\x1b[0m:~# ";
/** What the mock shell answers. */
const MOCK_SHELL: Partial<Record<string, string>> = {
  id: "uid=0(root) gid=0(root) groups=0(root)",
  whoami: "root",
  hostname: "attack-box",
  "ip -br a": "lo      UNKNOWN 127.0.0.1/8\r\neth0    UP      10.20.0.2/24",
};

/** An open shell: the `terminal_open` arguments its output goes to, and the line being typed. */
type Shell = { args: Args; line: string };

let terminalSessions = 0;
const terminals = new Map<number, Shell>();

const output = (t: Shell, data: string) => {
  send(t.args, { kind: "data", data }, "events");
};
const exited = (t: Shell) => {
  send(t.args, { kind: "exit", code: 0 }, "events");
};

/** What the shell answers to the line `line`. */
function reply(line: string): string {
  const command = line.trim();
  if (!command) return "";
  return MOCK_SHELL[command] ?? `${command.split(" ")[0]}: command not found (dev mock)`;
}

/** One key typed in `t`; false once it ends the session. */
function typeKey(session: number, t: Shell, ch: string): boolean {
  if (ch === "\r" && t.line.trim() === "exit") {
    terminals.delete(session);
    output(t, "\r\nlogout\r\n");
    exited(t);
    return false;
  }
  if (ch === "\r") {
    const answer = reply(t.line);
    output(t, `\r\n${answer ? `${answer}\r\n` : ""}${PROMPT}`);
    t.line = "";
  } else if (ch === "\x7f") {
    if (t.line) output(t, "\b \b");
    t.line = t.line.slice(0, -1);
  } else {
    t.line += ch;
    output(t, ch);
  }
  return true;
}

const write: BrowserHandler = (args) => {
  const session = Number(args?.session);
  const t = terminals.get(session);
  if (!t) return fail("the shell has ended");
  for (const ch of argText(args?.data)) if (!typeKey(session, t, ch)) break;
  return null;
};

/** The terminal commands, and the window a lab's shell opens in. */
export const terminalHandlers: Record<string, BrowserHandler> = {
  terminal_open: (args) => {
    const session = ++terminalSessions;
    const t = { args, line: "" };
    terminals.set(session, t);
    output(t, "Connected to the attack box (dev mock).\r\n" + PROMPT);
    return session;
  },
  terminal_write: write,
  terminal_resize: () => null,
  terminal_close: (args) => {
    const t = terminals.get(Number(args?.session));
    terminals.delete(Number(args?.session));
    if (t) exited(t);
    return null;
  },
  terminal_window: (args) => {
    const q = new URLSearchParams({ id: argText(args?.id), runtime: argText(args?.runtime) });
    openWindow(`/shell?${q.toString()}`);
    return null;
  },
};
