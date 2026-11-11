"use client";

import { useEffect, type RefObject } from "react";
import { describeError } from "@/lib/errors";
import { screenConnect, type ScreenConnection } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

/** Where the screen window stands: asking for the console, showing it (VNC), handing it to the
 *  system's client (RDP), refused (why: the machine has no screen, it isn't running…), or its
 *  connection lost (why) or closed. */
export type ScreenState =
  | { kind: "connecting" }
  | { kind: "showing" }
  | { kind: "rdp"; connection: ScreenConnection }
  | { kind: "unavailable"; message: string }
  | { kind: "failed"; message: string }
  | { kind: "ended" };

/** What the window needs of noVNC's client (`RFB`): it draws the screen into a target element
 *  and says how its connection goes. */
type Viewer = EventTarget & {
  scaleViewport: boolean;
  resizeSession: boolean;
  focusOnClick: boolean;
  disconnect: () => void;
  sendCredentials: (c: { password: string }) => void;
};
type ViewerClass = new (target: HTMLElement, url: string, options: { credentials?: { password: string }; wsProtocols?: string[] }) => Viewer;

/** noVNC's client, loaded with the window (its module reads the browser at load). */
const loadViewer = async (): Promise<ViewerClass> => (await import("@/vendor/novnc/core/rfb.js")).default;

/** Connects the screen window to its machine's screen, again each time `attempt` changes: asks the
 *  core for it, then draws a VNC screen into `host` through the window's bridge, or says how to
 *  open a Remote Desktop. Disconnects when the window goes or tries again. */
export function useScreen(host: RefObject<HTMLDivElement | null>, attempt: number, setState: (s: ScreenState) => void) {
  const t = useT();
  useEffect(() => {
    let viewer: Viewer | null = null;
    let gone = false;
    // Read through a call: the window may go while the core answers or noVNC loads.
    const ended = () => gone;
    const start = async () => {
      let connection: ScreenConnection;
      try {
        connection = await screenConnect();
      } catch (e) {
        if (!ended()) setState({ kind: "unavailable", message: describeError(e, t) });
        return;
      }
      if (ended()) return;
      if (connection.protocol === "rdp") {
        setState({ kind: "rdp", connection });
        return;
      }
      const target = host.current;
      if (!target || !connection.url) {
        setState({ kind: "failed", message: t("shell.vmScreen.noViewer") });
        return;
      }
      const Viewer = await loadViewer();
      if (ended()) return;
      const password = connection.password ?? "";
      const v = new Viewer(target, connection.url, { credentials: { password }, wsProtocols: ["binary"] });
      viewer = v;
      v.scaleViewport = true;
      v.resizeSession = false;
      v.focusOnClick = true;
      v.addEventListener("connect", () => {
        setState({ kind: "showing" });
      });
      v.addEventListener("credentialsrequired", () => {
        v.sendCredentials({ password });
      });
      v.addEventListener("securityfailure", () => {
        setState({ kind: "failed", message: t("shell.vmScreen.securityFailure") });
      });
      v.addEventListener("disconnect", (e) => {
        if (ended()) return;
        const clean = (e as CustomEvent<{ clean: boolean }>).detail.clean;
        setState(clean ? { kind: "ended" } : { kind: "failed", message: t("shell.vmScreen.lost") });
      });
    };
    start().catch((e: unknown) => {
      if (!ended()) setState({ kind: "failed", message: describeError(e, t) });
    });
    return () => {
      gone = true;
      viewer?.disconnect();
    };
  }, [host, attempt, setState, t]);
}
