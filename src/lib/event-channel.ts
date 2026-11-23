import { emit } from "@tauri-apps/api/event";

/** What a channel's decoder answers for a payload it can't read: the event is dropped. */
export const DROP: unique symbol = Symbol("dropped");

/**
 * A Tauri event and the one decoder of its payload: an event's name and the shape of what it
 * carries are declared once, together, so no listener reads an `unknown` payload by hand and a
 * sender can only send what the listeners read. `decode` answers the payload as `T`, or [`DROP`]
 * for anything else (an older or another window's shape), which no handler then sees.
 */
export type Channel<T> = { readonly name: string; readonly decode: (payload: unknown) => T | typeof DROP };

/** The channel of event `name`, whose payload `decode` reads. */
export function channel<T>(name: string, decode: (payload: unknown) => T | typeof DROP): Channel<T> {
  return { name, decode };
}

/** A channel whose event carries nothing: "something changed, read it again". Any payload is
 *  ignored. */
export function signal(name: string): Channel<undefined> {
  return channel(name, () => undefined);
}

/** Broadcasts `payload` on `ch` to every window of the app. */
export function send<T>(ch: Channel<T>, ...payload: T extends undefined ? [] : [T]): Promise<void> {
  return emit(ch.name, payload[0]);
}
