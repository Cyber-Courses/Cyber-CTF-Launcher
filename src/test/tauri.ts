import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { answer } from "@/lib/dev-mock";
import type { CommandError } from "@/lib/errors";

export type Handler = (args: Record<string, unknown> | undefined) => unknown;
export interface Call {
  cmd: string;
  args: Record<string, unknown> | undefined;
}

type Stream = { onmessage?: (m: unknown) => void };

/** Sends `messages` down a streaming command's channel (its `logs` or `events` argument). */
export function stream(args: Record<string, unknown> | undefined, ...messages: unknown[]) {
  const channel = (args?.logs ?? args?.events) as Stream | undefined;
  for (const m of messages) channel?.onmessage?.(m);
}

/** A streaming command that logs `lines` and then succeeds. */
const streams =
  (...lines: string[]): Handler =>
  (args) => {
    stream(args, ...lines);
    return null;
  };

// Commands the browser dev mock leaves unanswered (null), with a plausible answer for tests.
const DEFAULTS: Partial<Record<string, Handler>> = {
  installed_tools: () => [{ dependency: "vagrant", at: Math.floor(Date.now() / 1000) - 3600 }],
  shared_folder_get: () => ({ enabled: false, path: "/Users/player/CyberCTF", mountPoint: "/shared" }),
  shared_folder_set: (a) => ({ enabled: !!a?.enabled, path: typeof a?.path === "string" ? a.path : "", mountPoint: "/shared" }),
  oci_config: () => ({ configured: false, tenancy: "", region: "" }),
  azure_subscriptions: () => [{ name: "Pay-As-You-Go", id: "sub-1", isDefault: true }],
  gcp_billing_accounts: () => [{ id: "billing-1", name: "My billing", open: true }],
  gcp_organizations: () => [],
  gcp_account: () => "player@example.com",
  server_save: (a) => ({ ...(a?.input as object), id: (a?.input as { id?: string } | undefined)?.id ?? "new-host", password: undefined }),
  server_public_key: () => "ssh-ed25519 AAAAC3Nza cyberctf",
  machine_storage_clean: () => 1_000_000_000,
  lab_launch: streams("==> api: Creating", "✓ Lab ready"),
  lab_stop: streams("==> api: Stopping"),
  lab_park: streams("==> DC01: Saving state"),
  lab_resume: streams("==> DC01: Resuming"),
  lab_provision: streams("TASK [Configure DC01]"),
  attack_vm_start: streams("==> attack: Booting"),
  attack_vm_stop: streams("==> attack: Halting"),
  exegol_start: streams("Starting attack box"),
  exegol_stop: streams("Stopping attack box"),
  install_dependency: streams("Downloading", "Installed"),
  uninstall_dependency: streams("Removing", "Removed"),
  install_vagrant_plugin: streams("Installing plugin"),
  aws_login: streams("Opening browser"),
  cloud_login: streams("Opening browser"),
};

/**
 * Answers the Tauri commands the screens call, for component tests in jsdom: the browser dev
 * mock's sample data (a signed-in player, running labs, a server, an AWS account), with
 * per-test `overrides`. A handler that throws rejects the invoke, like a failed Rust command.
 * Returns the list of calls made, for assertions.
 */
export function installTauri(overrides: Partial<Record<string, Handler>> = {}): Call[] {
  const calls: Call[] = [];
  mockWindows("main");
  mockIPC(
    (cmd, args) => {
      const a = args as Record<string, unknown> | undefined;
      calls.push({ cmd, args: a });
      const handler = overrides[cmd] ?? DEFAULTS[cmd];
      return handler ? handler(a) : answer(cmd, a);
    },
    { shouldMockEvents: true },
  );
  return calls;
}

/** Fails a mocked Tauri command as the Rust core does: its invoke rejects with `reason` as sent
 *  (the error's code and message, or a plain string), never with an `Error`. */
export function rejectWith(reason: unknown): never {
  throw reason;
}

/** A command that fails as the Rust core does: rejected with its error's code and message. */
export const rejects =
  (error: CommandError): Handler =>
  () =>
    rejectWith(error);

/** The names of the commands invoked, in order. */
export const commands = (calls: Call[]) => calls.map((c) => c.cmd);
