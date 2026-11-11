"use client";

import type { ReactNode } from "react";
import { KeyRound, Network } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Segmented } from "@/components/ui/segmented";
import { PROVIDER_NAMES } from "@/lib/tauri";
import { Field, Input, Note, Step } from "@/features/servers/host-setup/form";
import { BackNav, StepError } from "@/features/servers/host-setup/step-nav";
import { Spinner } from "@/components/ui/spinner";
import type { SignIn } from "@/features/servers/host-setup/setup-model";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { warn } from "@/lib/failure";
import { pickFile } from "@/lib/pickers";
import { useT } from "@/lib/i18n";

// Proxmox's sign-in step: SSH key (the launcher then makes its own API token on the node), API
// token, or user and password.

/** Proxmox's second connection step: how the launcher signs in, SSH key first. */
export function ProxmoxSignIn({ s, signInFields, authorizeKey }: { s: HostSetup; signInFields: ReactNode; authorizeKey: ReactNode }) {
  const t = useT();
  const { connectionOk } = s;
  const { back } = s.nav;
  const { signInMethod, setSignInMethod } = s.login;
  const { connect, connecting, connectError } = s.connect;
  const methods: { value: SignIn; label: string }[] = [
    { value: "sshKey", label: t("servers.setup.connection.methodSshKey") },
    { value: "token", label: t("servers.setup.connection.methodToken") },
    { value: "password", label: t("servers.setup.connection.methodPassword") },
  ];
  return (
    <Step
      icon={Network}
      title={t("servers.setup.connection.title", { kind: PROVIDER_NAMES.proxmox })}
      description={t("servers.setup.connection.descriptionSignIn")}
    >
      <Segmented label={t("servers.setup.connection.method")} value={signInMethod} onChange={setSignInMethod} options={methods} className="self-start" />
      {signInMethod === "sshKey" ? <SshKeyFields s={s} /> : signInFields}
      {signInMethod === "token" && authorizeKey}
      <StepError error={connectError} />
      <BackNav onBack={back}>
        <Button onClick={connect} disabled={!connectionOk || connecting}>
          {connecting ? (
            <>
              <Spinner className="size-4" /> {t("servers.setup.nav.continue")}
            </>
          ) : (
            t("servers.setup.nav.continue")
          )}
        </Button>
      </BackNav>
    </Step>
  );
}

/** The player's private key (picked or typed: its path only), the SSH user and port, and what
 *  the launcher will do with them on save. */
function SshKeyFields({ s }: { s: HostSetup }) {
  const t = useT();
  const { sshKey, setSshKey } = s.login;
  const pick = () => {
    pickFile({ defaultPath: sshKey.path || undefined, title: t("servers.setup.connection.sshKeyPick") })
      .then((path) => {
        if (path !== null) setSshKey("path", path);
      })
      .catch(warn("choosing an SSH key"));
  };
  return (
    <>
      <div className="mt-4 grid gap-3 sm:grid-cols-[1fr_auto]">
        <Field label={t("servers.setup.connection.sshKeyPath")} hint={t("servers.setup.connection.sshKeyPathHint")}>
          <Input
            value={sshKey.path}
            onChange={(e) => {
              setSshKey("path", e.target.value);
            }}
            placeholder="~/.ssh/id_ed25519"
          />
        </Field>
        <div className="sm:pt-[1.3rem]">
          <Button variant="outline" onClick={pick}>
            {t("servers.setup.connection.sshKeyChoose")}
          </Button>
        </div>
      </div>
      <div className="mt-3 grid gap-3 sm:grid-cols-[1fr_6.875rem]">
        <Field label={t("servers.setup.connection.sshUser")}>
          <Input
            value={sshKey.user}
            onChange={(e) => {
              setSshKey("user", e.target.value);
            }}
            placeholder="root"
          />
        </Field>
        <Field label={t("servers.setup.connection.sshPort")}>
          <Input
            type="number"
            min={1}
            max={65535}
            value={Number.isNaN(sshKey.port) ? "" : sshKey.port}
            onChange={(e) => {
              setSshKey("port", e.target.value ? Number(e.target.value) : Number.NaN);
            }}
            placeholder="22"
          />
        </Field>
      </div>
      <SshKeyNote />
    </>
  );
}

/** What saving with an SSH key does on the node, said before it happens. */
function SshKeyNote() {
  const t = useT();
  return (
    <Note className="mt-3">
      <div className="flex items-center gap-1.5 text-[0.8125rem] font-medium">
        <KeyRound className="size-3.5 text-muted-foreground" /> {t("servers.setup.connection.sshKeyNoteTitle")}
      </div>
      <p className="mt-1 text-[0.75rem] text-muted-foreground">
        {t.rich("servers.setup.connection.sshKeyNoteBody", {
          code: (c) => <code className="mx-1 rounded-xs bg-glass-2 px-1 py-0.5 font-mono">{c}</code>,
        })}
      </p>
    </Note>
  );
}
