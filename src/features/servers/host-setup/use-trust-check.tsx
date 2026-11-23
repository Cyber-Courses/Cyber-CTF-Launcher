"use client";

import { useRef, useState } from "react";
import { serverIdentity, serverTrustSshKeys, type ServerHostInput, type ServerIdentity } from "@/lib/tauri";
import { TrustDialog, type Unconfirmed } from "@/features/servers/host-setup/trust-dialog";

// Trust on first use for a Proxmox server: before the launcher signs in to it, the player is shown
// its self-signed certificate's fingerprint and its node's SSH host keys, to compare with what the
// server itself shows, and confirms them. The certificate's is saved with the host; the keys go to
// the launcher's own known_hosts. A server that shows the confirmed ones goes on without a question.

/** A fingerprint as the core writes it, to compare with one typed or stored in any case. */
const normal = (f: string | null | undefined) => (f ?? "").toUpperCase().replace(/[^0-9A-F]/g, "");

/** What of `identity` the payload `v` hasn't confirmed; null when it is all confirmed. */
function unconfirmed(v: ServerHostInput, identity: ServerIdentity): Unconfirmed | null {
  const pinned = v.tlsFingerprint ?? null;
  const tls = identity.tls !== null && normal(identity.tls) !== normal(pinned) ? { presented: identity.tls, pinned: normal(pinned) ? pinned : null } : null;
  const ssh = identity.ssh && identity.ssh.state !== "confirmed" ? identity.ssh : null;
  return tls || ssh ? { tls, ssh, sshError: identity.sshError } : null;
}

/** Asks the player to confirm what a Proxmox server offers, when it isn't confirmed yet: `ensure`
 *  resolves to the payload with the confirmed certificate, or null when the player cancelled.
 *  `dialog` is the question, rendered by the step. */
export function useTrustCheck(setV: React.Dispatch<React.SetStateAction<ServerHostInput>>) {
  const [asking, setAsking] = useState<Unconfirmed | null>(null);
  const answer = useRef<((yes: boolean) => void) | null>(null);
  const decide = (yes: boolean) => {
    setAsking(null);
    answer.current?.(yes);
    answer.current = null;
  };
  const ensure = async (v: ServerHostInput): Promise<ServerHostInput | null> => {
    if (v.provider !== "proxmox") return v;
    const open = unconfirmed(v, await serverIdentity(v));
    if (!open) return v;
    const yes = await new Promise<boolean>((resolve) => {
      answer.current = resolve;
      setAsking(open);
    });
    if (!yes) return null;
    if (open.ssh)
      await serverTrustSshKeys(
        v,
        open.ssh.keys.map((k) => k.fingerprint),
      );
    const confirmed = { ...v, tlsFingerprint: open.tls?.presented ?? v.tlsFingerprint ?? null };
    setV((s) => ({ ...s, tlsFingerprint: confirmed.tlsFingerprint }));
    return confirmed;
  };
  const dialog = asking ? (
    <TrustDialog
      asking={asking}
      onConfirm={() => {
        decide(true);
      }}
      onCancel={() => {
        decide(false);
      }}
    />
  ) : null;
  return { ensure, dialog };
}
