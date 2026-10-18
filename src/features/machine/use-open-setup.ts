"use client";

import { machineOpenSetup } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** Opens the "Set up this machine" window, at `step` when given; a failure is told. */
export function useOpenSetup() {
  const t = useT();
  return (step?: string) => {
    machineOpenSetup(step).catch(tell(t("machine.errors.openSetup")));
  };
}
