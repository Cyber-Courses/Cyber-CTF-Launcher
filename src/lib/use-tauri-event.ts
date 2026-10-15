"use client";

import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { ignore } from "@/lib/failure";

/**
 * Calls `handler` with the payload of each Tauri `event` while mounted and `enabled`. The latest
 * `handler` is used without listening again. The payload is whatever the emitter sent (the Rust
 * side or another window): the handler reads it as it must be, and ignores anything else.
 */
export function useTauriEvent(event: string, handler: (payload: unknown) => void, enabled = true) {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => {
    if (!enabled) return;
    const off = listen(event, (e) => {
      latest.current(e.payload);
    });
    return () => {
      // Tauri's unlisten answers a promise (its type says void): chained, so its failure is caught.
      off.then((unlisten) => Promise.resolve().then(unlisten)).catch(ignore("the listener was never set up"));
    };
  }, [event, enabled]);
}
