"use client";

import { Suspense, useCallback, useRef, useState } from "react";
import { useSearchParams } from "next/navigation";
import { KeyRound, Monitor, MonitorOff, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Toaster } from "@/components/ui/toaster";
import { DevMockGate } from "@/components/dev-mock-gate";
import { WindowEndedBar } from "@/components/window-ended-bar";
import { WindowTitleBar } from "@/components/window-title-bar";
import { tell } from "@/lib/failure";
import { toast } from "@/lib/toast";
import { useT } from "@/lib/i18n";
import { screenOpenRdp, screenPassword, type ScreenConnection } from "@/lib/tauri";
import { useScreen, type ScreenState } from "@/app/screen/use-screen";
import { SignInPopover } from "@/app/screen/sign-in";

/** A lab machine's screen window (opened by `screen_window`): its VNC screen drawn by noVNC through
 *  the window's own loopback bridge, or its Remote Desktop opened in the system's client. */
function Screen() {
  const t = useT();
  const machine = useSearchParams().get("machine") ?? "";
  const host = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<ScreenState>({ kind: "connecting" });
  const [attempt, setAttempt] = useState(0);
  useScreen(host, attempt, setState);
  const retry = useCallback(() => {
    setState({ kind: "connecting" });
    setAttempt((a) => a + 1);
  }, []);
  const vnc = state.kind === "connecting" || state.kind === "showing" || state.kind === "failed" || state.kind === "ended";
  return (
    <div className="flex h-screen flex-col bg-background text-foreground">
      <WindowTitleBar toolbar className="border-b border-border">
        <span data-tauri-drag-region className="mr-auto truncate font-mono text-[0.75rem] text-muted-foreground">
          {machine} · {t(`shell.vmScreen.state.${state.kind}`)}
        </span>
        <SignInPopover />
      </WindowTitleBar>
      <div className="relative min-h-0 flex-1 bg-black">
        <div ref={host} className={vnc ? "h-full w-full" : "hidden"} data-testid="screen-host" />
        {state.kind === "connecting" && (
          <Centered>
            <Spinner className="size-5" /> {t("shell.vmScreen.connecting")}
          </Centered>
        )}
        {state.kind === "unavailable" && <Unavailable message={state.message} onRetry={retry} />}
        {state.kind === "rdp" && <RemoteDesktop connection={state.connection} />}
        {(state.kind === "failed" || state.kind === "ended") && <EndedBar state={state} onRetry={retry} />}
      </div>
      <Toaster />
    </div>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 bg-background p-6 text-center text-[0.8125rem] text-muted-foreground">
      {children}
    </div>
  );
}

/** The machine has no screen to show, and why (the core's words). */
function Unavailable({ message, onRetry }: { message: string; onRetry: () => void }) {
  const t = useT();
  return (
    <Centered>
      <MonitorOff className="size-6" />
      <p className="font-medium text-foreground">{t("shell.vmScreen.unavailable")}</p>
      <p className="max-w-md">{message}</p>
      <Button variant="outline" size="sm" onClick={onRetry}>
        <RotateCw className="size-3.5" /> {t("shell.vmScreen.retry")}
      </Button>
    </Centered>
  );
}

/** A Remote Desktop: where it answers, the system's client to open it in (or what to install), and
 *  its password copied on request. The window holds the connection (a tunnel, for a cloud's) open. */
function RemoteDesktop({ connection }: { connection: ScreenConnection }) {
  const t = useT();
  const copyPassword = () => {
    screenPassword()
      .then((p) => navigator.clipboard.writeText(p))
      .then(() => {
        toast(t("shell.vmScreen.passwordCopied"));
      })
      .catch(tell(t("errors.clipboard")));
  };
  return (
    <Centered>
      <Monitor className="size-6" />
      <p className="font-medium text-foreground">{t("shell.vmScreen.rdpTitle")}</p>
      <p className="max-w-md">{connection.client ? t("shell.vmScreen.rdpHint", { client: connection.client }) : t("shell.vmScreen.noClient")}</p>
      <p className="font-mono text-[0.75rem]">
        {connection.address}
        {connection.user ? ` · ${connection.user}` : ""}
      </p>
      {connection.note && <p className="max-w-md text-[0.75rem]">{connection.note}</p>}
      {connection.tunnelled && <p className="max-w-md text-[0.75rem]">{t("shell.vmScreen.tunnelled")}</p>}
      <div className="flex gap-2">
        {connection.client && (
          <Button variant="primary" size="sm" onClick={() => void screenOpenRdp().catch(tell(t("shell.vmScreen.openFailed")))}>
            <Monitor className="size-3.5" /> {t("shell.vmScreen.openRdp", { client: connection.client })}
          </Button>
        )}
        {connection.hasPassword && (
          <Button variant="outline" size="sm" onClick={copyPassword}>
            <KeyRound className="size-3.5" /> {t("shell.vmScreen.copyPassword")}
          </Button>
        )}
      </div>
    </Centered>
  );
}

/** Over a lost or closed VNC screen: why, Try again and Close. */
function EndedBar({ state, onRetry }: { state: Extract<ScreenState, { kind: "failed" | "ended" }>; onRetry: () => void }) {
  const t = useT();
  const failed = state.kind === "failed";
  return <WindowEndedBar failed={failed} message={failed ? state.message : t("shell.vmScreen.closed")} retry={t("shell.vmScreen.retry")} onRetry={onRetry} />;
}

export default function ScreenWindow() {
  return (
    <DevMockGate>
      <Suspense>
        <Screen />
      </Suspense>
    </DevMockGate>
  );
}
