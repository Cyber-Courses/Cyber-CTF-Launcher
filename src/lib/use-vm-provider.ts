"use client";

import { useEffect, useState } from "react";
import { getVmProvider, VM_PROVIDER_CHANGED, VM_PROVIDER_KEY } from "@/lib/settings";
import type { Provider } from "@/lib/tauri";

/** The hypervisor labs run on (Settings' choice, null = automatic), kept current as it changes
 *  anywhere in the app: in this window (`VM_PROVIDER_CHANGED`) or another one, the Settings or
 *  the setup window (a `storage` event for its key; a null key is the storage cleared). */
export function useVmProvider(): Provider | null {
  const [provider, setProvider] = useState<Provider | null>(() => getVmProvider());
  useEffect(() => {
    const read = () => {
      setProvider(getVmProvider());
    };
    const onStorage = (e: StorageEvent) => {
      if (e.key === null || e.key === VM_PROVIDER_KEY) read();
    };
    window.addEventListener(VM_PROVIDER_CHANGED, read);
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener(VM_PROVIDER_CHANGED, read);
      window.removeEventListener("storage", onStorage);
    };
  }, []);
  return provider;
}
