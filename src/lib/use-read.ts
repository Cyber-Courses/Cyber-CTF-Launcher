"use client";

import { useEffect, useRef, useState } from "react";
import { warn } from "@/lib/failure";
import { describeError } from "@/lib/errors";

/** An async read as the page shows it: nothing asked (`idle`), asked and not answered
 *  (`reading`), its value (`read`), or why it couldn't be read (`failed`). One state at a time:
 *  a value and an error, or "reading" and a value, can't be shown together. */
export type Read<T> = { state: "idle" } | { state: "reading" } | { state: "read"; value: T } | { state: "failed"; error: string };

/** The value of `read`, or null while there is none (idle, reading, failed). */
export function valueOf<T>(read: Read<T>): T | null {
  return read.state === "read" ? read.value : null;
}

/**
 * Reads `read()` for `key` (null: nothing to read) and answers it as a [`Read`]. The answer is
 * kept with the key it was asked for and shown only while that key is current, so a page that
 * moved to another lab never shows the previous lab's answer, a slow answer for an old key never
 * replaces a newer one, and nothing is set once unmounted. A new key reads again; the latest
 * `read` is used without reading again. A failure is logged as `what` and kept as its reason.
 */
export function useRead<T>(key: string | null, read: () => Promise<T>, what: string): Read<T> {
  const latest = useRef(read);
  useEffect(() => {
    latest.current = read;
  });
  const [answer, setAnswer] = useState<{ key: string; read: Read<T> } | null>(null);
  useEffect(() => {
    if (key === null) return;
    let live = true;
    latest
      .current()
      .then((value) => {
        if (live) setAnswer({ key, read: { state: "read", value } });
      })
      .catch((e: unknown) => {
        warn(what)(e);
        if (live) setAnswer({ key, read: { state: "failed", error: describeError(e) } });
      });
    return () => {
      live = false;
    };
  }, [key, what]);
  if (key === null) return { state: "idle" };
  return answer?.key === key ? answer.read : { state: "reading" };
}
