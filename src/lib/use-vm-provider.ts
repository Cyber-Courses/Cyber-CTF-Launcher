"use client";

import { useEffect, useState } from "react";
import { getVmProvider, VM_PROVIDER_CHANGED } from "@/lib/settings";
import type { Provider } from "@/lib/tauri";

/** The hypervisor labs run on (Settings' choice, null = automatic), kept current as it changes
 *  anywhere in the app. */
export function useVmProvider(): Provider | null {
  const [provider, setProvider] = useState<Provider | null>(() => getVmProvider());
  useEffect(() => {
    const read = () => {
      setProvider(getVmProvider());
    };
    window.addEventListener(VM_PROVIDER_CHANGED, read);
    return () => {
      window.removeEventListener(VM_PROVIDER_CHANGED, read);
    };
  }, []);
  return provider;
}
