"use client";

import { useState } from "react";
import type { Tab } from "@/components/shell/shell-model";

/** The lab to open in Labs (null: the list). `tick` changes on every navigation, so asking for
 *  the same lab again still opens it. */
export type OpenLab = { slug: string | null; tick: number };

/** The open screen, and the lab Labs should open. */
export function useNavigation() {
  const [tab, setTab] = useState<Tab>("home");
  const [openLab, setOpenLab] = useState<OpenLab>({ slug: null, tick: 0 });

  function navigate(next: Tab, slug?: string) {
    // Setup now lives inside the Machine dashboard; "setup" just lands on Machine.
    setTab(next === "setup" ? "machine" : next);
    setOpenLab((o) => ({ slug: next === "setup" ? null : (slug ?? null), tick: o.tick + 1 }));
  }

  /** Labs, with its search field focused. */
  function findALab() {
    navigate("labs");
    requestAnimationFrame(() => document.querySelector<HTMLInputElement>("[data-lab-search]")?.focus());
  }

  return { tab, setTab, openLab, navigate, findALab };
}
