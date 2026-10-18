// How the dev mock answers a command, as the Rust core does: its arguments, the channel it
// streams on, its failure, and the time it takes.
import type { CommandError } from "@/lib/errors";
import type { Args } from "@/lib/dev-mock/types";

/** A streaming command's channel (its `logs` or `events` argument), as the mock sees it. */
type Stream = { onmessage?: (m: unknown) => void };

/** Sends `m` down a streaming command's `channel` argument, if it has one. */
export function send(args: Args, m: unknown, channel: "logs" | "events" = "logs"): void {
  (args?.[channel] as Stream | undefined)?.onmessage?.(m);
}

/** An argument as text: a string or a number as is, anything else (missing, an object) as
 *  `fallback`. */
export function argText(value: unknown, fallback = ""): string {
  if (typeof value === "string") return value;
  return typeof value === "number" ? String(value) : fallback;
}

/** Fails a command as the Rust core does: its invoke rejects with the error's code and message
 *  (a plain object, never an `Error`). */
export function fail(message: string, code: CommandError["code"] = "other"): never {
  const error: unknown = { code, message };
  throw error;
}

/** Waits `ms`, as a command taking its time. */
export const sleep = (ms: number) =>
  new Promise((r) => {
    setTimeout(r, ms);
  });
