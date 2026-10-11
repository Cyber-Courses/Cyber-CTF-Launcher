"use client";

import { useState } from "react";
import type { LabAction } from "@/features/labs/lab-detail/lab-state";
import type { Park } from "@/lib/tauri";

/** The page's own actions on a built lab (pause, shut down, resume, provision again), and which
 *  one is in flight, so its button (not the others) reads as busy. */
export function usePageActions({
  onPark,
  onResume,
  onProvision,
}: {
  onPark?: (mode: Park) => Promise<unknown> | void;
  onResume?: () => Promise<unknown> | void;
  onProvision?: (machine: string | null) => Promise<unknown> | void;
}) {
  const [acting, setActing] = useState<LabAction | null>(null);
  // Which machine to provision again ("" = all), from the Details panel.
  const [provisionTarget, setProvisionTarget] = useState("");
  const act = async (what: LabAction) => {
    setActing(what);
    try {
      if (what === "resume") await onResume?.();
      else if (what === "provision") await onProvision?.(provisionTarget || null);
      else await onPark?.(what);
    } finally {
      setActing(null);
    }
  };
  return { acting, act, provisionTarget, setProvisionTarget };
}
