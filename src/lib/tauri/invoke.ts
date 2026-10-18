import { Channel, invoke, type InvokeArgs } from "@tauri-apps/api/core";

/** Invokes a command that answers nothing (a Rust `()`, which arrives as `null`). */
export async function invokeUnit(command: string, args?: InvokeArgs): Promise<void> {
  await invoke(command, args);
}

/**
 * Invokes a command that streams back while it runs over a Tauri channel, passed as the
 * command's `logs` argument (log lines) or `events` argument (test steps, terminal output).
 * `onMessage` takes what the command sends, as the wrapper calling this declares it.
 */
export function invokeStreaming<R = void>(
  command: string,
  args: Record<string, unknown>,
  onMessage: (message: never) => void,
  channel: "logs" | "events" = "logs",
): Promise<R> {
  const stream = new Channel<never>();
  stream.onmessage = onMessage;
  return invoke<R>(command, { ...args, [channel]: stream });
}
