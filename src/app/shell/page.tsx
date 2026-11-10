"use client";

import "@xterm/xterm/css/xterm.css";
import { Suspense, useCallback, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { RotateCw, SquareTerminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Toaster } from "@/components/ui/toaster";
import { DevMockGate } from "@/components/dev-mock-gate";
import { WindowTitleBar } from "@/components/window-title-bar";
import { tell, warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { labAttackShell, type Runtime } from "@/lib/tauri";
import { oneOf } from "@/lib/local-store";
import { useTerminalSession, type ShellState } from "@/app/shell/use-terminal-session";

/** The runtimes a lab's shell window is opened with: every `Runtime` (a new one fails to compile here). */
const RUNTIMES = Object.keys({ DOCKER: true, VM: true, CLOUD: true } satisfies Record<Runtime, true>) as Runtime[];

/** The attack box shell window (opened by `terminal_window`, which names the lab and its runtime
 *  in the address): its session, or why there is none. */
function Shell() {
  const params = useSearchParams();
  const id = params.get("id");
  const runtime = oneOf(params.get("runtime"), RUNTIMES);
  return id && runtime ? <Session id={id} runtime={runtime} /> : <NoLab />;
}

/** A window opened without a lab it knows: nothing to connect to. */
function NoLab() {
  const t = useT();
  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <WindowTitleBar toolbar className="border-b border-border">
        <span data-tauri-drag-region className="mr-auto truncate font-mono text-[0.75rem] text-muted-foreground">
          {t("shell.terminal.failed")}
        </span>
      </WindowTitleBar>
      <p role="alert" className="m-auto px-6 text-center text-[0.8125rem] text-destructive">
        {t("shell.terminal.noLab")}
      </p>
    </div>
  );
}

/** xterm.js over the pseudo-terminal of lab `id`'s attack box. */
function Session({ id, runtime }: { id: string; runtime: Runtime }) {
  const t = useT();
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<ShellState>({ kind: "connecting" });
  const [attempt, setAttempt] = useState(0);

  useTerminalSession(host, { id, runtime, attempt }, setState);

  const reconnect = useCallback(() => {
    setState({ kind: "connecting" });
    setAttempt((a) => a + 1);
  }, []);
  // The system terminal, for players who prefer theirs; a failure says why.
  const openOutside = () => void labAttackShell(id, runtime).catch(tell(t("shell.terminal.systemTerminalFailed")));

  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <TitleBar state={state} onOpenOutside={openOutside} />
      <div className="relative min-h-0 flex-1 p-2">
        <div ref={host} className="h-full w-full" />
        {(state.kind === "ended" || state.kind === "failed") && <EndedBar state={state} onReconnect={reconnect} />}
      </div>
      <Toaster />
    </div>
  );
}

/** The window's title bar, with the session's state and a button that opens the system
 *  terminal instead. */
function TitleBar({ state, onOpenOutside }: { state: ShellState; onOpenOutside: () => void }) {
  const t = useT();
  return (
    <WindowTitleBar toolbar className="border-b border-border">
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
    </WindowTitleBar>
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
        <Button
          variant="ghost"
          size="sm"
          onClick={() => {
            getCurrentWindow().close().catch(warn("closing the window"));
          }}
        >
          {t("shell.terminal.close")}
        </Button>
      </div>
    </div>
  );
}

export default function ShellWindow() {
  return (
    <DevMockGate>
      <Suspense>
        <Shell />
      </Suspense>
    </DevMockGate>
  );
}
