"use client";

import { useRef, useState } from "react";
import { ConsentDialog } from "@/components/ui/consent-dialog";
import { serverIdentity, type ServerHostInput, type ServerIdentity } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

// Trust on first use for a Proxmox server: before the launcher signs in to it, the player is shown
// its self-signed certificate's fingerprint, to compare with what the server itself shows, and
// confirms it. The fingerprint is saved with the host. A server that shows the confirmed one goes
// on without a question.

/** A fingerprint as the core writes it, to compare with one typed or stored in any case. */
const normal = (f: string | null | undefined) => (f ?? "").toUpperCase().replace(/[^0-9A-F]/g, "");

/** What the server offers that isn't confirmed yet: its certificate's fingerprint, and the one
 *  confirmed before if it changed. */
export type Unconfirmed = {
  tls: { presented: string; pinned: string | null } | null;
};

/** What of `identity` the payload `v` hasn't confirmed; null when it is all confirmed. */
export function unconfirmed(v: ServerHostInput, identity: ServerIdentity): Unconfirmed | null {
  const pinned = v.tlsFingerprint ?? null;
  const tls = identity.tls !== null && normal(identity.tls) !== normal(pinned) ? { presented: identity.tls, pinned: normal(pinned) ? pinned : null } : null;
  return tls ? { tls } : null;
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

/** The question: the fingerprint in full, where to find the server's own to compare, and a
 *  warning when it changed. */
function TrustDialog({ asking, onConfirm, onCancel }: { asking: Unconfirmed; onConfirm: () => void; onCancel: () => void }) {
  const t = useT();
  const changed = !!asking.tls?.pinned;
  return (
    <ConsentDialog
      title={changed ? t("servers.setup.trust.titleChanged") : t("servers.setup.trust.title")}
      confirmLabel={changed ? t("servers.setup.trust.confirmChanged") : t("servers.setup.trust.confirm")}
      cancelLabel={t("servers.setup.trust.cancel")}
      onConfirm={onConfirm}
      onCancel={onCancel}
    >
      <p>{changed ? t("servers.setup.trust.changedBody") : t("servers.setup.trust.body")}</p>
      {asking.tls && (
        <section className="mt-3" aria-label={t("servers.setup.trust.certificate")}>
          <p className="font-medium text-foreground">{t("servers.setup.trust.certificate")}</p>
          <p className="text-[0.75rem]">{t("servers.setup.trust.certificateWhere")}</p>
          {asking.tls.pinned && <Fingerprint label={t("servers.setup.trust.previous")} value={asking.tls.pinned} />}
          <Fingerprint label={t("servers.setup.trust.sha256")} value={asking.tls.presented} />
        </section>
      )}
    </ConsentDialog>
  );
}

function Fingerprint({ label, value }: { label: string; value: string }) {
  return (
    <div className="mt-1.5">
      <span className="block text-[0.6875rem] uppercase tracking-wide">{label}</span>
      <code className="surface-log block break-all rounded-sm px-2 py-1 font-mono text-[0.6875rem] text-foreground">{value}</code>
    </div>
  );
}
