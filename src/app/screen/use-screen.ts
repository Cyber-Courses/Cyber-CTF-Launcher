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

/** How long the viewer gets to show the screen once it starts connecting: past it, the window
 *  says the screen didn't answer instead of spinning (the bridge closes a silent one sooner). */
export const SHOW_WITHIN_MS = 30_000;

/** noVNC's client, loaded with the window (its module reads the browser at load). */
const loadViewer = async (): Promise<ViewerClass> => (await import("@/vendor/novnc/core/rfb.js")).default;

/** What the window's states are told of a viewer's life, and how to tell whether the window went. */
type Watch = { setState: (s: ScreenState) => void; ended: () => boolean; password: string; noAnswer: string; lost: string; refused: string };

/** Follows viewer `v`'s connection into the window's state: showing once it connects, or, when it
 *  never does within `SHOW_WITHIN_MS` or is closed first, where the screen didn't answer (it is
 *  disconnected then). Gives what stops the wait. */
function follow(v: Viewer, w: Watch): () => void {
  let shown = false;
  let timedOut = false;
  const timer = setTimeout(() => {
    if (shown || w.ended()) return;
    timedOut = true;
    v.disconnect();
    w.setState({ kind: "failed", message: w.noAnswer });
  }, SHOW_WITHIN_MS);
  v.addEventListener("connect", () => {
    shown = true;
    clearTimeout(timer);
    w.setState({ kind: "showing" });
  });
  v.addEventListener("credentialsrequired", () => {
    v.sendCredentials({ password: w.password });
  });
  v.addEventListener("securityfailure", () => {
    w.setState({ kind: "failed", message: w.refused });
  });
  v.addEventListener("disconnect", (e) => {
    clearTimeout(timer);
    if (w.ended() || timedOut) return;
    const clean = (e as CustomEvent<{ clean: boolean }>).detail.clean;
    // Closed before the screen ever showed: it didn't answer (the bridge closes such a one).
    if (!shown) w.setState({ kind: "failed", message: w.noAnswer });
    else w.setState(clean ? { kind: "ended" } : { kind: "failed", message: w.lost });
  });
  return () => {
    clearTimeout(timer);
  };
}

/** Connects the screen window to its machine's screen, again each time `attempt` changes: asks the
 *  core for it, then draws a VNC screen into `host` through the window's bridge, or says how to
 *  open a Remote Desktop. Disconnects when the window goes or tries again. */
export function useScreen(host: RefObject<HTMLDivElement | null>, attempt: number, setState: (s: ScreenState) => void) {
  const t = useT();
  useEffect(() => {
    let viewer: Viewer | null = null;
    let gone = false;
    let stopWaiting = () => {};
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
      // Why the screen never showed: where it didn't answer, when the core said where it is.
      const noAnswer = connection.address ? t("shell.vmScreen.noAnswer", { address: connection.address }) : t("shell.vmScreen.noAnswerAnywhere");
      const v = new Viewer(target, connection.url, { credentials: { password }, wsProtocols: ["binary"] });
      viewer = v;
      v.scaleViewport = true;
      v.resizeSession = false;
      v.focusOnClick = true;
      stopWaiting = follow(v, { setState, ended, password, noAnswer, lost: t("shell.vmScreen.lost"), refused: t("shell.vmScreen.securityFailure") });
    };
    start().catch((e: unknown) => {
      if (!ended()) setState({ kind: "failed", message: describeError(e, t) });
    });
    return () => {
      gone = true;
      stopWaiting();
      viewer?.disconnect();
    };
  }, [host, attempt, setState, t]);
}
