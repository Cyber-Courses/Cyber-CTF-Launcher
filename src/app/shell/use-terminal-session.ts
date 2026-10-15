import { useEffect, type Dispatch, type RefObject, type SetStateAction } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { ignore } from "@/lib/failure";
import { terminalClose, terminalOpen, terminalResize, terminalWrite, type Runtime, type ShellKind } from "@/lib/tauri";
import { describeError } from "@/lib/errors";

/** Reads a CSS custom property from the page, so the terminal follows the app's theme. */
const cssVar = (name: string, fallback: string) =>
  (typeof window === "undefined" ? "" : getComputedStyle(document.documentElement).getPropertyValue(name).trim()) || fallback;

export type ShellState = { kind: "connecting" } | { kind: "open" } | { kind: "ended"; code: number | null } | { kind: "failed"; message: string };

type SetShellState = Dispatch<SetStateAction<ShellState>>;
type ShellTarget = { id: string; kind: ShellKind; runtime: Runtime };

/** An xterm.js terminal in `element`, in the app's theme, fitted to it (and refitted as it
 *  resizes) and focused, with what takes it down. */
function mountTerminal(element: HTMLDivElement): { term: Terminal; unmount: () => void } {
  const term = new Terminal({
    cursorBlink: true,
    fontFamily: cssVar("--font-mono", "ui-monospace, monospace"),
    fontSize: 13,
    lineHeight: 1.2,
    scrollback: 5000,
    theme: {
      background: cssVar("--background", "#09090b"),
      foreground: cssVar("--foreground", "#f4f4f6"),
      cursor: cssVar("--foreground", "#f4f4f6"),
      selectionBackground: "rgba(148, 163, 184, 0.35)",
    },
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  term.open(element);
  fit.fit();
  term.focus();
  const observer = new ResizeObserver(() => {
    fit.fit();
  });
  observer.observe(element);
  return {
    term,
    unmount: () => {
      observer.disconnect();
      term.dispose();
    },
  };
}

/** Opens a pseudo-terminal on `target` behind `term`: keys and resizes go to it, its output comes
 *  back, and its state goes to `setState`. Returns what closes it. */
function connectTerminal(term: Terminal, target: ShellTarget, setState: SetShellState): () => void {
  let session: number | null = null;
  let disposed = false;
  const close = (s: number) => void terminalClose(s).catch(ignore("the session ends with the window"));
  const input = term.onData((d) => {
    if (session !== null) void terminalWrite(session, d).catch(ignore("a key lost on a closing session"));
  });
  const resized = term.onResize(({ cols, rows }) => {
    if (session !== null) void terminalResize(session, cols, rows).catch(ignore("the next resize sends the size again"));
  });
  terminalOpen({ ...target, cols: term.cols, rows: term.rows }, (e) => {
    // Output still arriving after Reconnect or the window closing: its terminal is gone.
    if (disposed) return;
    if (e.kind === "data") term.write(e.data);
    else {
      session = null;
      setState({ kind: "ended", code: e.code });
    }
  })
    .then((s) => {
      if (disposed) {
        close(s);
        return;
      }
      session = s;
      setState((prev) => (prev.kind === "connecting" ? { kind: "open" } : prev));
    })
    .catch((err: unknown) => {
      if (!disposed) setState({ kind: "failed", message: describeError(err) });
    });
  return () => {
    disposed = true;
    input.dispose();
    resized.dispose();
    if (session !== null) close(session);
  };
}

/**
 * An xterm.js terminal in `host`, wired to a pseudo-terminal on the shell `id`. A new `attempt`
 * opens a fresh session (Reconnect); the session's state (connecting, open, ended, failed) goes
 * to `setState`.
 */
export function useTerminalSession(
  host: RefObject<HTMLDivElement | null>,
  { id, kind, runtime, attempt }: ShellTarget & { attempt: number },
  setState: SetShellState,
) {
  useEffect(() => {
    if (!host.current || !id) return;
    const { term, unmount } = mountTerminal(host.current);
    const disconnect = connectTerminal(term, { id, kind, runtime }, setState);
    return () => {
      disconnect();
      unmount();
    };
  }, [host, id, kind, runtime, attempt, setState]);
}
