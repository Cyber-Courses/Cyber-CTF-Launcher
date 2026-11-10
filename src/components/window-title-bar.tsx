"use client";

import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { ignore, warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { useWindowFrame } from "@/lib/platform";
import { cn } from "@/lib/utils";

type Props = {
  /** The title, translated. Without one, Linux and Windows show the window's own (what its system
   *  frame would) and macOS none (the system's title is hidden there). */
  title?: string;
  /** The bar also holds the page's own controls (`children`), so it stays where the browser
   *  draws the window (the preview outside the app), as a toolbar without title or window buttons. */
  toolbar?: boolean;
  children?: ReactNode;
  /** Borders and the like. */
  className?: string;
};

/**
 * The window's title bar, where the page draws it (see `useWindowFrame`). It drags the window,
 * and a double-click on it maximizes or restores the window: both are Tauri's, for any
 * `data-tauri-drag-region` with nothing clickable over it. On macOS it clears the traffic lights;
 * on Linux and Windows, where the window has no system frame, it shows the title and the
 * minimize, maximize/restore and close buttons, in the app's appearance. Nothing in a browser,
 * unless it is a `toolbar`.
 */
export function WindowTitleBar({ title, toolbar = false, children, className }: Props) {
  const frame = useWindowFrame();
  if (frame === "system" && !toolbar) return null;
  const custom = frame === "linux" || frame === "windows";
  return (
    <div
      data-tauri-drag-region
      className={cn(
        "relative flex h-11 shrink-0 select-none items-center gap-1",
        frame === "overlay" ? "pr-3 pl-20" : custom ? "pr-1.5 pl-4" : "pr-3 pl-4",
        className,
      )}
    >
      {frame !== "system" && <Title given={title} own={custom} inline={toolbar} />}
      {toolbar && (
        <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center justify-end gap-1">
          {children}
        </div>
      )}
      {custom && <WindowControls edge={frame === "linux"} />}
    </div>
  );
}

/** The title: `given`, else the window's own when `own`. Centred over the bar, or after its
 *  start when `inline` (a toolbar, whose controls would run under a centred one). */
function Title({ given, own, inline }: { given?: string; own: boolean; inline: boolean }) {
  const title = useWindowTitle(own && given === undefined) ?? given;
  if (!title) return null;
  return (
    <span
      className={cn(
        "pointer-events-none truncate text-[0.8125rem] font-medium",
        inline ? "max-w-[40%] shrink-0 text-muted-foreground" : "absolute inset-x-40 text-center",
      )}
    >
      {title}
    </span>
  );
}

/** The window's own title (set where it is opened, src-tauri/src/app/window.rs), when `read`. */
function useWindowTitle(read: boolean) {
  const [title, setTitle] = useState<string | null>(null);
  useEffect(() => {
    if (!read) return;
    let live = true;
    getCurrentWindow()
      .title()
      .then((t) => {
        if (live) setTitle(t);
      })
      .catch(ignore("the bar shows no title; the window works the same"));
    return () => {
      live = false;
    };
  }, [read]);
  return title;
}

/** Whether the window is maximized, kept up to date as it is resized (maximize, restore, a
 *  double-click on the bar, the window manager's own shortcuts). */
function useMaximized() {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    const win = getCurrentWindow();
    let live = true;
    const read = () => {
      win
        .isMaximized()
        .then((m) => {
          if (live) setMaximized(m);
        })
        .catch(ignore("the maximize button keeps its last state; it toggles the same"));
    };
    read();
    const unlisten = win.onResized(read);
    return () => {
      live = false;
      unlisten
        .then((stop) => {
          stop();
        })
        .catch(ignore("the window is going away with its listener"));
    };
  }, []);
  return maximized;
}

const BUTTON =
  "flex h-8 w-10 items-center justify-center rounded-control text-muted-foreground transition-colors hover:bg-glass-2 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

/** Minimize, maximize/restore and close. Close asks the window to close, as the system's
 *  button does: the quit guard (src-tauri/src/app/quit.rs) can still hold it while a lab deploys.
 *  With `edge`, a hairline around the window (Linux: Windows draws its own, with the shadow). */
function WindowControls({ edge }: { edge: boolean }) {
  const t = useT();
  const maximized = useMaximized();
  const win = () => getCurrentWindow();
  const maximizeLabel = maximized ? t("shell.window.restore") : t("shell.window.maximize");
  return (
    <>
      <div className="ml-auto flex shrink-0 items-center gap-0.5">
        <button
          type="button"
          className={BUTTON}
          aria-label={t("shell.window.minimize")}
          title={t("shell.window.minimize")}
          onClick={() => void win().minimize().catch(warn("minimizing the window"))}
        >
          <Minus className="size-4" />
        </button>
        <button
          type="button"
          className={BUTTON}
          aria-label={maximizeLabel}
          title={maximizeLabel}
          onClick={() => void win().toggleMaximize().catch(warn("maximizing or restoring the window"))}
        >
          {maximized ? <Copy className="size-3.5 -scale-x-100" data-icon="restore" /> : <Square className="size-3.5" data-icon="maximize" />}
        </button>
        <button
          type="button"
          className={cn(BUTTON, "hover:bg-destructive hover:text-white")}
          aria-label={t("shell.window.close")}
          title={t("shell.window.close")}
          onClick={() => void win().close().catch(warn("closing the window"))}
        >
          <X className="size-4" />
        </button>
      </div>
      {/* The window's edge, which no system frame draws on Linux: a hairline in the
          appearance's border colour, gone when maximized. */}
      {edge && !maximized && <div aria-hidden className="pointer-events-none fixed inset-0 z-[60] border border-border" />}
    </>
  );
}
