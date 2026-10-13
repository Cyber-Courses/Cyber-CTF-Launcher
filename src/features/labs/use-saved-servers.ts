"use client";

import { useEffect, useState } from "react";
import { serverList, type ServerHost } from "@/lib/tauri";

/** The saved servers (read once), where a VM lab can also run: for a lab row's "needs setup". */
export function useSavedServers() {
  const [servers, setServers] = useState<ServerHost[]>([]);
  useEffect(() => {
    serverList()
      .then((l) => setServers(l.hosts))
      .catch(() => setServers([]));
  }, []);
  return servers;
}
