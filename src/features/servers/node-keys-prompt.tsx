"use client";

import { serverNodeHostKeys, serverTrustNodeHostKeys, type SshHostKeys } from "@/lib/tauri";
import { createAsk } from "@/lib/ask";
import { TrustDialog } from "@/features/servers/host-setup/trust-dialog";

// A lab on a Proxmox server whose lab bridge is internal to the node runs through the node, which
// is checked against the SSH host keys the player confirmed, never trusted on first use. A server
// set up before those were confirmed has none: before such a run (and at Test connection) the
// player confirms them once, in the server setup's own question. `NodeKeysPrompt` is mounted once,
// in the app shell; closing it is a no.
const { ask, answer, useAsked } = createAsk<SshHostKeys, boolean>(false);

/** The last question asked, so the next waits for its answer (Test all, two starts at once). */
let queue: Promise<unknown> = Promise.resolve();

/** Whether a lab may run through saved server `id`'s node: its keys are the confirmed ones (or
 *  its labs don't go through it), or the player confirmed them now. False when the player
 *  declined. Rejects when the node offers other keys than the confirmed ones
 *  (`sshHostKeyRefused`), or they couldn't be read. */
export function confirmNodeKeys(id: string): Promise<boolean> {
  const turn = queue.then(async () => {
    const keys = await serverNodeHostKeys(id);
    if (!keys) return true;
    if (!(await ask(keys))) return false;
    await serverTrustNodeHostKeys(
      id,
      keys.keys.map((k) => k.fingerprint),
    );
    return true;
  });
  queue = turn.catch(() => undefined);
  return turn;
}

export function NodeKeysPrompt() {
  const asked = useAsked();
  if (!asked) return null;
  return (
    <TrustDialog
      key={asked.id}
      asking={{ tls: null, ssh: asked.question, sshError: null }}
      onConfirm={() => {
        answer(true);
      }}
      onCancel={() => {
        answer(false);
      }}
    />
  );
}
