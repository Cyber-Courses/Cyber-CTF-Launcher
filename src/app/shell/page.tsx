"use client";

import "@xterm/xterm/css/xterm.css";
import { Suspense, useCallback, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RotateCw, SquareTerminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useT } from "@/lib/i18n";
import { useIsMac } from "@/lib/platform";
import { labAttackShell, attackVmShell, type Runtime, type ShellKind } from "@/lib/tauri";
import { useTerminalSession, type ShellState } from "@/app/shell/use-terminal-session";
import { installDevMock } from "@/lib/dev-mock";

// Development only: ?mock in a plain browser answers the Tauri commands with sample data.
installDevMock();

/** The attack box shell window (opened by `terminal_window`): xterm.js over a pseudo-terminal. */
function Shell() {
  const params = useSearchParams();
  const id = params.get("id") ?? "";
  const kind = (params.get("kind") ?? "lab") as ShellKind;
  const runtime = (params.get("runtime") ?? "DOCKER") as Runtime;
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<ShellState>({ kind: "connecting" });
  const [attempt, setAttempt] = useState(0);

  useTerminalSession(host, { id, kind, runtime, attempt }, setState);

  const reconnect = useCallback(() => {
    setState({ kind: "connecting" });
    setAttempt((a) => a + 1);
  }, []);
  // The system terminal, for players who prefer theirs.
  const openOutside = () => void (kind === "attackVm" ? attackVmShell(id) : labAttackShell(id, runtime)).catch(() => {});

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <TitleBar state={state} onOpenOutside={openOutside} />
      <div className="relative min-h-0 flex-1 p-2">
        <div ref={host} className="h-full w-full" />
        {(state.kind === "ended" || state.kind === "failed") && <EndedBar state={state} onReconnect={reconnect} />}
      </div>
    </div>
  );
}

/** Overlay title bar: drags the window and clears the macOS traffic lights. Shows the session's
 *  state and opens the system terminal instead. */
function TitleBar({ state, onOpenOutside }: { state: ShellState; onOpenOutside: () => void }) {
  const t = useT();
  // macOS overlays the traffic lights on this bar; elsewhere the window has its own title bar.
  const isMac = useIsMac();
  return (
    <div
      data-tauri-drag-region
      className={`flex h-11 shrink-0 select-none items-center justify-end gap-1 border-b border-border pr-3 ${isMac ? "pl-20" : "pl-4"}`}
    >
      <span data-tauri-drag-region className="mr-auto truncate font-mono text-[0.75rem] text-muted-foreground">
        {state.kind === "connecting"
          ? t("shell.terminal.connecting")
          : state.kind === "open"
            ? t("shell.terminal.open")
            : state.kind === "ended"
              ? t("shell.terminal.ended")
              : t("shell.terminal.failed")}
      </span>
      <Button variant="ghost" size="sm" onClick={onOpenOutside} title={t("shell.terminal.systemTerminalHint")}>
        <SquareTerminal className="size-3.5" /> {t("shell.terminal.systemTerminal")}
      </Button>
    </div>
  );
}

/** Under an ended or failed session: why, Reconnect and Close. */
function EndedBar({ state, onReconnect }: { state: Extract<ShellState, { kind: "ended" | "failed" }>; onReconnect: () => void }) {
  const t = useT();
  return (
    <div className="absolute inset-x-0 bottom-0 flex items-center justify-between gap-3 border-t border-border bg-background/95 px-4 py-2.5 text-[0.8125rem]">
      <span className={state.kind === "failed" ? "text-destructive" : "text-muted-foreground"}>
        {state.kind === "failed" ? state.message : state.code ? t("shell.terminal.shellEndedWithCode", { code: state.code }) : t("shell.terminal.shellEnded")}
      </span>
      <div className="flex gap-2">
        <Button variant="outline" size="sm" onClick={onReconnect}>
          <RotateCw className="size-3.5" /> {t("shell.terminal.reconnect")}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => void getCurrentWindow().close()}>
          {t("shell.terminal.close")}
        </Button>
      </div>
    </div>
  );
}

export default function ShellWindow() {
  return (
    <Suspense>
      <Shell />
    </Suspense>
  );
}
