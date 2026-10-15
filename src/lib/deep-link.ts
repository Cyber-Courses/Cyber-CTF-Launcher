import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { useEffect, useRef } from "react";
import { warn } from "@/lib/failure";

const SLUG = /^[a-z0-9]+(-[a-z0-9]+)*$/;

/** The lab slug in a `cyberctf://labs/<slug>` link, or null for anything else. */
function labSlugFrom(url: string): string | null {
  try {
    const u = new URL(url);
    if (u.protocol !== "cyberctf:" || u.hostname !== "labs") return null;
    const slug = u.pathname.replace(/^\/+|\/+$/g, "");
    return SLUG.test(slug) ? slug : null;
  } catch {
    return null;
  }
}

/** The first lab slug among a batch of opened links. */
export function firstLabSlug(urls: string[] | null): string | null {
  return urls?.map(labSlugFrom).find((s): s is string => s !== null) ?? null;
}

// The link the app was launched with is read once per session: `getCurrent()` keeps returning
// it for as long as the app runs, so reading it again (a screen mounting again, a reload) would
// keep pulling the player back into that lab.
let launchLinkRead = false;

/** Whether this is the first read of the launch link this session (and marks it read). */
export function takeLaunchLink(): boolean {
  if (launchLinkRead) return false;
  launchLinkRead = true;
  return true;
}

/**
 * Calls `onLab` with the lab a `cyberctf://labs/<slug>` link asks for: the link that opened the
 * app (once), then any link opened while it runs. Mounted by the main window's shell, so a link
 * works whatever screen is open. The lab is only shown, never started: a web page must not be
 * able to start containers without a click.
 */
export function useLabLinks(onLab: (slug: string) => void, enabled = true) {
  const handler = useRef(onLab);
  useEffect(() => {
    handler.current = onLab;
  });
  useEffect(() => {
    if (!enabled) return;
    let unlisten: (() => void) | undefined;
    let alive = true;
    const take = (urls: string[] | null) => {
      const slug = firstLabSlug(urls);
      if (slug && alive) handler.current(slug);
    };
    if (takeLaunchLink()) getCurrent().then(take).catch(warn("reading the link the app was opened with"));
    onOpenUrl(take)
      .then((fn) => {
        if (alive) unlisten = fn;
        else fn();
      })
      .catch(warn("listening for deep links"));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [enabled]);
}
