"use client";

import { useEffect, useRef } from "react";

/** Re-read the machine when the window regains focus: the user may have just installed Docker or
 *  a hypervisor in the setup window (or externally), and the setup status would otherwise stay
 *  stale until a restart. A ref keeps the handler current without re-subscribing each render. */
export function useRefreshOnFocus(onRefresh: () => void | Promise<void>) {
  const refresh = useRef(onRefresh);
  useEffect(() => {
    refresh.current = onRefresh;
  }, [onRefresh]);
  useEffect(() => {
    const onFocus = () => void refresh.current();
    window.addEventListener("focus", onFocus);
    return () => {
      window.removeEventListener("focus", onFocus);
    };
  }, []);
}
