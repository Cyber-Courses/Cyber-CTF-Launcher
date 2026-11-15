import { act } from "@testing-library/react";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { answer } from "@/lib/dev-mock";
import type { CommandError } from "@/lib/errors";
import { PROXMOX_PLACEMENT } from "@/lib/dev-mock/proxmox-sample";
import { destructive, warnings } from "@/lib/confirm";
import { translate } from "@/lib/i18n";
import { backendCommands, callable } from "@/test/backend-commands";
import { recordUncallable } from "@/test/uncallable";
import { resetInFlight, track, waitForAnswers } from "@/test/in-flight";
import { isOperation, replay, resetRunService, startOperation, watchLabs } from "@/lib/dev-mock/run-service";
import { SAMPLE_IN_FLIGHT } from "@/lib/dev-mock/labs";
import type { ActiveOperation } from "@/lib/tauri/run-events";

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

/** `active_operations`, the operations already running when the core started (the run events a
 *  window replays first: see `run-service`): a deploy worker's launch on each of `ids`, as the
 *  backend reports a lab it deploys (`busy_labs` lists them too). */
export const deployingOps =
  (ids: () => string[]): Handler =>
  () =>
    ids().map((labId) => ({ labId, op: "launch", machine: null, deploying: true, step: null, runtime: "DOCKER" }));

/** The system dialog `confirmDestructive` asks in, answered with its confirm button (`yes`) or
 *  its Cancel: `installTauri({ "plugin:dialog|message": answersDialog(false) })`. A dialog that
 *  only says something (`warnInDialog`) is closed with its Ok. */
export const answersDialog =
  (yes: boolean): Handler =>
  (args) => {
    const buttons = args?.buttons as { OkCancelCustom: [string, string] } | undefined;
    if (!buttons) return "Ok";
    const [ok, cancel] = buttons.OkCancelCustom;
    return yes ? ok : cancel;
  };

/** The dialogs asked (their title and message), from the calls `installTauri` returned. */
export const dialogsAsked = (calls: Call[]) =>
  calls.filter((c) => c.cmd === "plugin:dialog|message").map((c) => ({ title: c.args?.title, message: c.args?.message }));

// Commands the browser dev mock leaves unanswered (null), with a plausible answer for tests.
export const DEFAULTS: Partial<Record<string, Handler>> = {
  // The player confirms what they asked for, unless a test answers otherwise.
  "plugin:dialog|message": answersDialog(true),
  installed_tools: () => [{ dependency: "vagrant", at: Math.floor(Date.now() / 1000) - 3600 }],
  shared_folder_get: () => ({ enabled: false, path: "/Users/player/CyberCTF", mountPoint: "/shared" }),
  shared_folder_set: (a) => ({ enabled: !!a?.enabled, path: typeof a?.path === "string" ? a.path : "", mountPoint: "/shared" }),
  oci_config: () => ({ configured: false, tenancy: "", region: "" }),
  azure_subscriptions: () => [{ name: "Pay-As-You-Go", id: "sub-1", isDefault: true }],
  gcp_billing_accounts: () => [{ id: "billing-1", name: "My billing", open: true }],
  gcp_organizations: () => [],
  gcp_account: () => "player@example.com",
  server_save: (a) => ({ ...(a?.input as object), id: (a?.input as { id?: string } | undefined)?.id ?? "new-host", password: undefined }),
  // The node made the launcher's token: saved as a token setup that remembers the key.
  server_save_ssh_key: (a) => {
    const input = a?.input as { id?: string | null } | undefined;
    return { ...(input as object), id: input?.id ?? "new-host", username: "root@pam!cyberctf-0a1b2c3d", password: undefined };
  },
  server_default_ssh_key: () => "/Users/me/.ssh/id_ed25519",
  // The player's Proxmox server as its API lists it: one node, two storages, two bridges.
  server_proxmox_placement: () => PROXMOX_PLACEMENT,
  server_public_key: () => "ssh-ed25519 AAAAC3Nza cyberctf",
  // The server's certificate and node keys, confirmed already: no confirmation asked.
  server_identity: () => ({ tls: null, ssh: { pattern: "pve.lan", keys: [], state: "confirmed" }, sshError: null }),
  machine_storage_clean: () => 1_000_000_000,
  lab_launch: streams("==> api: Creating", "✓ Lab ready"),
  lab_stop: streams("==> api: Stopping"),
  lab_park: streams("==> DC01: Saving state"),
  lab_resume: streams("==> DC01: Resuming"),
  lab_provision: streams("TASK [Configure DC01]"),
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
// The in-app confirmation and warning dialogs, answered as the system dialog's double answers
// (`plugin:dialog|message`, recorded as one of its calls): a test sees the same question and
// answers it the same way.
let stopAnswering: (() => void) | null = null;

function answerInAppDialogs(calls: Call[], handler: Handler | undefined) {
  stopAnswering?.();
  const reply = (args: Record<string, unknown>) => {
    calls.push({ cmd: "plugin:dialog|message", args });
    return Promise.resolve().then(() => handler?.(args));
  };
  const offConfirm = destructive.subscribe(() => {
    const open = destructive.pending();
    if (!open) return;
    const { title, message, confirm } = open.question;
    reply({ title, message, kind: "warning", buttons: { OkCancelCustom: [confirm, translate("ui.confirm.cancel")] } }).then(
      (label) => {
        destructive.answer(label === confirm);
      },
      () => {
        destructive.answer(false);
      },
    );
  });
  const offWarn = warnings.subscribe(() => {
    const open = warnings.pending();
    if (!open) return;
    const { title, message } = open.question;
    void reply({ title, message, kind: "warning" }).finally(() => {
      warnings.answer(null);
    });
  });
  stopAnswering = () => {
    offConfirm();
    offWarn();
  };
}

const REGISTERED = backendCommands();

/** The answers a test holds open on purpose (until it resolves them, or for good). */
const HELD = new WeakSet<Promise<unknown>>();

/** `handler`, whose answer the test holds open on purpose (it resolves it when it means to, or
 *  never): [`settled`] doesn't wait for it. */
export const held =
  (handler: Handler): Handler =>
  (args) => {
    const out = handler(args);
    if (out instanceof Promise) HELD.add(out);
    return out;
  };

/** Waits until every command invoked has its answer (the ones a test holds aside, see [`held`]),
 *  and the renders those answers cause have landed: waited for, never a fixed delay, which a
 *  loaded machine can outlast. Gives up after 15 s, so a test that holds an answer without
 *  saying so fails on its assertions rather than hanging. */
export async function settled(): Promise<void> {
  await act(() => waitForAnswers(0));
}

export function installTauri(overrides: Partial<Record<string, Handler>> = {}): Call[] {
  const calls: Call[] = [];
  resetInFlight();
  answerInAppDialogs(calls, overrides["plugin:dialog|message"] ?? DEFAULTS["plugin:dialog|message"]);
  mockWindows("main");
  resetRunService();
  const handle = (cmd: string, a: Record<string, unknown> | undefined) => {
    const handler = overrides[cmd] ?? DEFAULTS[cmd];
    const out: unknown = handler ? handler(a) : answer(cmd, a);
    if (out instanceof Promise && !HELD.has(out)) track(out);
    return out;
  };
  mockIPC(
    (cmd, args) => {
      const a = args as Record<string, unknown> | undefined;
      calls.push({ cmd, args: a });
      if (!callable(REGISTERED, cmd)) {
        recordUncallable(cmd);
        throw new Error(`\`${cmd}\` isn't a command the Rust core registers (src-tauri/build.rs)`);
      }
      // The core's run service: an operation answers its id, its log and end are run events.
      if (isOperation(cmd)) return startOperation(cmd, a, (withLogs) => handle(cmd, withLogs));
      if (cmd === "labs_watch" && !overrides.labs_watch) return watchLabs(a, handle);
      if (cmd === "run_events" && !overrides.run_events) {
        const scan = handle("running_labs", undefined);
        return replay(
          (a?.after as number | null) ?? null,
          (overrides.active_operations?.(undefined) as ActiveOperation[] | null | undefined) ?? SAMPLE_IN_FLIGHT,
          Array.isArray(scan) ? (scan as string[]) : [],
        );
      }
      return handle(cmd, a);
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
