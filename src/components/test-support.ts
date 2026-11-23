// Helpers the component and feature tests share. Not part of the app: only tests import them.

import { act } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { vi } from "vitest";
import { flush } from "@/test/ui";

/** `value`, or a failed test naming `what` when it is null or undefined (instead of a `!`). */
export function must<T>(value: T | null | undefined, what = "a value"): T {
  if (value === null || value === undefined) throw new Error(`expected ${what}, got ${String(value)}`);
  return value;
}

/** The window has the focus, so a failure the player asked about shows as a toast; the warning
 *  it logs is kept out of the test output. */
export function toastsShown() {
  vi.spyOn(document, "hasFocus").mockReturnValue(true);
  vi.spyOn(console, "warn").mockImplementation(() => {});
}

/** Emits the app event `name` with `payload`, then lets what it causes render (`ms`, as `flush`). */
export async function emitSettled(name: string, payload: unknown, ms?: number) {
  await act(() => emit(name, payload));
  await flush(ms);
}

/** Runs `start` inside `act` (the renders it causes are flushed) and hands back its promise, to
 *  await once the screen has answered. */
export function startInAct<T>(start: () => Promise<T>): Promise<T> {
  let started: Promise<T> | undefined;
  act(() => {
    started = start();
  });
  return must(started, "the promise `start` returned");
}
