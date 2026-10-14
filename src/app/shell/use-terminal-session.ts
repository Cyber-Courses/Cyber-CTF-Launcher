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

/**
 * An xterm.js terminal in `host`, wired to a pseudo-terminal on the shell `id`: keys and resizes
 * go to it, its output comes back. A new `attempt` opens a fresh session (Reconnect); the
 * session's state (connecting, open, ended, failed) goes to `setState`.
 */
export function useTerminalSession(
  host: RefObject<HTMLDivElement | null>,
  { id, kind, runtime, attempt }: { id: string; kind: ShellKind; runtime: Runtime; attempt: number },
  setState: Dispatch<SetStateAction<ShellState>>,
) {
  useEffect(() => {
    if (!host.current || !id) return;
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
    term.open(host.current);
    fit.fit();
    term.focus();

    let session: number | null = null;
    let disposed = false;
    const input = term.onData((d) => {
      if (session !== null) void terminalWrite(session, d).catch(() => {});
    });
    const resized = term.onResize(({ cols, rows }) => {
      if (session !== null) void terminalResize(session, cols, rows).catch(() => {});
    });
    const observer = new ResizeObserver(() => fit.fit());
    observer.observe(host.current);

    terminalOpen(id, kind, runtime, term.cols, term.rows, (e) => {
      if (e.kind === "data") term.write(e.data);
      else {
        session = null;
        if (!disposed) setState({ kind: "ended", code: e.code });
      }
    })
      .then((s) => {
        if (disposed) {
          void terminalClose(s).catch(ignore("the session ends with the window"));
          return;
        }
        session = s;
        setState((prev) => (prev.kind === "connecting" ? { kind: "open" } : prev));
      })
      .catch((err) => !disposed && setState({ kind: "failed", message: describeError(err) }));

    return () => {
      disposed = true;
      observer.disconnect();
      input.dispose();
      resized.dispose();
      if (session !== null) void terminalClose(session).catch(ignore("the session ends with the window"));
      term.dispose();
    };
  }, [host, id, kind, runtime, attempt, setState]);
}
