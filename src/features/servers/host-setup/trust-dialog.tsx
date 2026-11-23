"use client";

import { ConsentDialog } from "@/components/ui/consent-dialog";
import type { ServerIdentity } from "@/lib/tauri";
import { useT } from "@/lib/i18n";

// The question a Proxmox server's identity is confirmed with: in its setup (`use-trust-check`),
// and before a lab runs through a node whose SSH host keys were never confirmed
// (`node-keys-prompt`).

/** What the server offers that isn't confirmed yet: its certificate's fingerprint (and the one
 *  confirmed before, if it changed), and its node's keys when they are new or changed. */
export type Unconfirmed = {
  tls: { presented: string; pinned: string | null } | null;
  ssh: ServerIdentity["ssh"];
  sshError: string | null;
};

/** The question: each fingerprint in full, where to find the server's own to compare, and a
 *  warning when one changed. */
export function TrustDialog({ asking, onConfirm, onCancel }: { asking: Unconfirmed; onConfirm: () => void; onCancel: () => void }) {
  const t = useT();
  const changed = !!asking.tls?.pinned || asking.ssh?.state === "changed";
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
      {asking.ssh && (
        <section className="mt-3" aria-label={t("servers.setup.trust.sshKeys")}>
          <p className="font-medium text-foreground">{t("servers.setup.trust.sshKeys")}</p>
          <p className="text-[0.75rem]">{t("servers.setup.trust.sshWhere")}</p>
          {asking.ssh.keys.map((k) => (
            <Fingerprint key={k.fingerprint} label={k.algorithm} value={k.fingerprint} />
          ))}
        </section>
      )}
      {asking.sshError && <p className="mt-3 text-[0.75rem]">{t("servers.setup.trust.sshUnread", { error: asking.sshError })}</p>}
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
