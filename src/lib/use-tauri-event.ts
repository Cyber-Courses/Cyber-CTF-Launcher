"use client";

import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { ignore } from "@/lib/failure";
import { DROP, type Channel } from "@/lib/event-channel";

/**
 * Calls `handler` with the payload of each event on `ch` while mounted and `enabled`, as the
 * channel's decoder reads it; an event it can't read never reaches the handler. The latest
 * `handler` is used without listening again.
 */
export function useTauriEvent<T>(ch: Channel<T>, handler: (payload: T) => void, enabled = true) {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  const { name, decode } = ch;
  useEffect(() => {
    if (!enabled) return;
    const off = listen(name, (e) => {
      const payload = decode(e.payload);
      if (payload !== DROP) latest.current(payload);
    });
    return () => {
      // Tauri's unlisten answers a promise (its type says void): chained, so its failure is caught.
      off.then((unlisten) => Promise.resolve().then(unlisten)).catch(ignore("the listener was never set up"));
    };
  }, [name, decode, enabled]);
}
