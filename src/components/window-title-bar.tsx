"use client";

import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { ignore, warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { useWindowFrame } from "@/lib/platform";
import { cn } from "@/lib/utils";

type Props = {
  /** A title, translated, centred over the bar (Settings). None otherwise, as on macOS, where the
   *  system's title is hidden: the page says what the window is. */
  title?: string;
  /** The bar also holds the page's own controls (`children`), so it stays where the browser
   *  draws the window (the preview outside the app), without the window buttons. */
  toolbar?: boolean;
  children?: ReactNode;
  /** Borders and the like. */
  className?: string;
};

/**
 * A secondary window's header row (Settings, the setups, the shell, the onboarding, the crash
 * screen): it drags the window, and a double-click on it maximizes or restores the window, both
 * Tauri's, for any `data-tauri-drag-region` with nothing clickable over it. The same row on every
 * system, as macOS draws it: there the traffic lights sit over its start, and on Linux and Windows
 * (no system frame) `WindowControls` sit at its end. Nothing in a browser, unless it is a
 * `toolbar`. The main window's header is `ShellHeader`, which ends with the same controls.
 */
export function WindowTitleBar({ title, toolbar = false, children, className }: Props) {
  const frame = useWindowFrame();
  if (frame === "system" && !toolbar) return null;
  return (
    <div
      data-tauri-drag-region
      className={cn("relative flex h-11 shrink-0 select-none items-center gap-1 pr-3", frame === "overlay" ? "pl-20" : "pl-4", className)}
    >
      {title && <span className="pointer-events-none absolute inset-x-40 truncate text-center text-[0.8125rem] font-medium">{title}</span>}
      {toolbar && (
        <div data-tauri-drag-region className="flex min-w-0 flex-1 items-center justify-end gap-1">
          {children}
        </div>
      )}
      <WindowControls />
    </div>
  );
}

/** Whether the window is maximized, kept up to date as it is resized (maximize, restore, a
 *  double-click on the header, the window manager's own shortcuts). */
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
      // Tauri's unlisten answers a promise (its type says void): chained, so its failure is caught.
      unlisten.then((stop) => Promise.resolve().then(stop)).catch(ignore("the window is going away with its listener"));
    };
  }, []);
  return maximized;
}

/** Tauri resizes a window without a system frame from a band inside its edges, over the page
 *  (tauri-runtime-wry, undecorated_resizing.rs): on Linux 5px along every edge, on Windows the
 *  frame's height (4px at any scale) along the top, the shadow taking the other edges. None while
 *  maximized. The buttons start past it, their fill running under it to the window's corner. */
const RESIZE_BAND = "5px";

/** One button: 46px wide (the system's own caption buttons) and the header's height, square
 *  cornered in the window's corner. Its fill (`before`) also covers the resize band above it (and
 *  right of it on Linux), so it reaches the window's edge, and its icon stays centred on the whole
 *  header. */
const BUTTON = cn(
  "relative flex h-full w-11.5 items-center justify-center text-muted-foreground outline-none transition-colors",
  "before:absolute before:inset-0 before:-top-(--band) before:transition-colors",
  "hover:text-foreground focus-visible:text-foreground focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
);
const QUIET = "hover:before:bg-glass-2 active:before:bg-glass focus-visible:before:bg-glass-2";
const CLOSE = cn(
  "hover:text-window-close-ink hover:before:bg-window-close active:before:bg-window-close/85",
  "focus-visible:text-window-close-ink focus-visible:before:bg-window-close",
);

/** The icon's look: 1px strokes, as the system draws them, centred on the whole header (`--band`
 *  shifted the button down), over the fill (`relative`). */
const ICON = { className: "relative translate-y-[calc(var(--band)/-2)]", strokeWidth: 1, absoluteStrokeWidth: true };

function ControlButton({ label, className, onClick, children }: { label: string; className: string; onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" className={cn(BUTTON, className)} aria-label={label} title={label} onClick={onClick}>
      {children}
    </button>
  );
}

/**
 * Minimize, maximize/restore and close, at the end of a header row (whose box must be
 * `relative`), on Linux and Windows, where the window has no system frame; nothing on macOS (the
 * traffic lights) or in a browser. They fill the row's height in the window's top-right corner;
 * a drag-region spacer keeps the row's own content clear of them. Close asks the window to close,
 * as the system's button does: the quit guard (src-tauri/src/app/quit.rs) can still hold it while
 * a lab deploys. On Linux a hairline draws the window's edge, which no frame or shadow does there
 * (Windows keeps its own shadow and border).
 */
export function WindowControls() {
  const frame = useWindowFrame();
  if (frame !== "linux" && frame !== "windows") return null;
  return <Controls linux={frame === "linux"} />;
}

function Controls({ linux }: { linux: boolean }) {
  const t = useT();
  const maximized = useMaximized();
  const win = () => getCurrentWindow();
  const band = maximized ? "0px" : RESIZE_BAND;
  return (
    <>
      <div data-tauri-drag-region aria-hidden className={cn("shrink-0 self-stretch", linux && !maximized ? "w-[calc(8.625rem+5px)]" : "w-34.5")} />
      <div
        data-window-controls
        style={{ "--band": band } as CSSProperties}
        className={cn("absolute right-0 bottom-0 flex top-(--band)", linux && "right-(--band)")}
      >
        <ControlButton label={t("shell.window.minimize")} className={QUIET} onClick={() => void win().minimize().catch(warn("minimizing the window"))}>
          <Minus {...ICON} className={cn(ICON.className, "size-4")} />
        </ControlButton>
        <ControlButton
          label={maximized ? t("shell.window.restore") : t("shell.window.maximize")}
          className={QUIET}
          onClick={() => void win().toggleMaximize().catch(warn("maximizing or restoring the window"))}
        >
          {maximized ? (
            <Copy {...ICON} className={cn(ICON.className, "size-3.5 -scale-x-100")} data-icon="restore" />
          ) : (
            <Square {...ICON} className={cn(ICON.className, "size-3.5")} data-icon="maximize" />
          )}
        </ControlButton>
        <ControlButton
          label={t("shell.window.close")}
          className={cn(CLOSE, linux && "before:-right-(--band)")}
          onClick={() => void win().close().catch(warn("closing the window"))}
        >
          <X {...ICON} className={cn(ICON.className, "size-4")} />
        </ControlButton>
      </div>
      {/* The window's edge, which nothing draws on Linux: a hairline in the appearance's border
          colour, gone when maximized (the window then has no edge to show). */}
      {linux && !maximized && <div aria-hidden className="pointer-events-none fixed inset-0 z-[60] border border-border" />}
    </>
  );
}
