"use client";

import { machineOpenSetup, type Provider } from "@/lib/tauri";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** Opens the "Set up this machine" window, at `step` when given (for the hypervisor `provider`
 *  when given); a failure is told. */
export function useOpenSetup() {
  const t = useT();
  return (step?: string, provider?: Provider) => {
    machineOpenSetup(step, provider).catch(tell(t("machine.errors.openSetup")));
  };
}
