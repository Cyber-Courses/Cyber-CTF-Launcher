"use client";

import { useEffect } from "react";
import { labLaunchLocal } from "@/lib/tauri";
import { useT } from "@/lib/i18n";
import { tell } from "@/lib/failure";
import { notify } from "@/lib/notify";

/** GUI e2e only: when the `__wdio_launch_local__` localStorage key holds a lab-folder path, start that local lab
 *  exactly as the Import button does (the same `labLaunchLocal`, toasts and all), so an e2e can
 *  drive the local launch and its success/failure UI without the native folder picker. Inert unless
 *  a test sets the flag, which the shipped app never does. */
export function WdioLaunch() {
  const t = useT();
  useEffect(() => {
    let path: string | null = null;
    try {
      path = localStorage.getItem("__wdio_launch_local__");
      if (path) localStorage.removeItem("__wdio_launch_local__");
    } catch {
      /* no storage */
    }
    if (!path) return;
    void labLaunchLocal(path, () => {})
      .then(() => notify(t("labs.import.started")))
      .catch(tell(t("labs.import.failed")));
  }, [t]);
  return null;
}
