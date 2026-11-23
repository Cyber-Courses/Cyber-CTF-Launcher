// Events between the browser preview's tabs, as Tauri's event bus links the app's windows: an
// event emitted in one tab (Settings asking the main window to show the onboarding again, a
// server saved in the setup) reaches the listeners of every tab.
import { argText } from "@/lib/dev-mock/command";
import type { BrowserHandler } from "@/lib/dev-mock/live";

type Internals = { runCallback: (id: number, data: unknown) => void };

const eventListeners = new Map<string, number[]>();
let eventBus: BroadcastChannel | null = null;

/** Calls this tab's listeners of `event` with `payload`. */
function deliver(event: string, payload: unknown) {
  const internals = (window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__;
  for (const id of eventListeners.get(event) ?? []) internals.runCallback(id, { event, id, payload });
}

/** Joins the preview's event bus (once per tab), so events emitted in the other tabs arrive. */
export function joinEventBus() {
  if (typeof BroadcastChannel === "undefined" || eventBus) return;
  eventBus = new BroadcastChannel("cyberctf-dev-mock-events");
  eventBus.onmessage = (e: MessageEvent<{ event: string; payload: unknown }>) => {
    deliver(e.data.event, e.data.payload);
  };
}

const emit: BrowserHandler = (args) => {
  const event = argText(args?.event);
  deliver(event, args?.payload);
  eventBus?.postMessage({ event, payload: args?.payload });
  return null;
};

/** Tauri's event plugin: listen, unlisten, emit. */
export const eventHandlers: Record<string, BrowserHandler> = {
  "plugin:event|listen": (args) => {
    const event = argText(args?.event);
    const handler = Number(args?.handler);
    eventListeners.set(event, [...(eventListeners.get(event) ?? []), handler]);
    return handler;
  },
  "plugin:event|unlisten": (args) => {
    const event = argText(args?.event);
    eventListeners.set(
      event,
      (eventListeners.get(event) ?? []).filter((id) => id !== Number(args?.eventId)),
    );
    return null;
  },
  "plugin:event|emit": emit,
  "plugin:event|emit_to": emit,
};
