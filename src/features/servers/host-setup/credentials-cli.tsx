"use client";

import { CheckCircle2 } from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import { Field, Input, Select } from "@/features/servers/host-setup/form";
import { CheckingNote, SignInLog, SignInNote, SignedInNote } from "@/features/servers/host-setup/credentials-parts";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

// The clouds that sign in through their own CLI (a browser sign-in): Azure, Google Cloud and
// AWS with CLI credentials. Each shows the sign-in, its log, then what the account gives access to.

/** Azure: the CLI sign-in, then the subscription it gives access to (or one pasted). */
export function AzureCredentials({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, azureSubs } = s;
  return (
    <div className="space-y-3">
      {s.azureChecking ? (
        <CheckingNote>{t("servers.setup.credentials.checkingAzure")}</CheckingNote>
      ) : azureSubs.length > 0 ? (
        <>
          {/* The sign-in, its log, then the subscription it gives access to (as for GCP). */}
          <SignedInNote s={s}>
            <span className="text-muted-foreground">{t("servers.setup.credentials.signedInAzure")}</span>
          </SignedInNote>
          <SignInLog s={s} />
          <Field label={t("servers.setup.credentials.subscription")}>
            <Select value={v.username} onChange={(e) => s.set("username", e.target.value)}>
              {!v.username && <option value="">{t("servers.setup.credentials.chooseSubscription")}</option>}
              {azureSubs.map((sub) => (
                <option key={sub.id} value={sub.id}>
                  {sub.name}
                  {sub.isDefault ? ` ${t("servers.setup.credentials.isDefault")}` : ""}
                </option>
              ))}
            </Select>
          </Field>
        </>
      ) : (
        <>
          <SignInNote
            hint={<span className="text-muted-foreground">{t("servers.setup.credentials.azureSignInHint")}</span>}
            label={t("servers.setup.credentials.azureSignIn")}
            onSignIn={s.signIn}
            signingIn={s.signingIn}
          />
          <SignInLog s={s} />
          <Field label={t("servers.setup.credentials.subscriptionId")} hint={t("servers.setup.credentials.orPaste")}>
            <Input {...s.text("username")} placeholder="00000000-0000-0000-0000-000000000000" />
          </Field>
        </>
      )}
    </div>
  );
}

/** Google Cloud: the CLI sign-in, then the billing account and organization for the lab projects. */
export function GcpCredentials({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, gcpBilling } = s;
  return (
    <div className="space-y-3">
      {/* A list that couldn't be read: the others still show, and the reason does too. */}
      {s.gcpError && !s.gcpChecking && (
        <p role="alert" className="text-[0.75rem] break-words text-destructive">
          {t("servers.setup.credentials.gcpReadFailed", { error: s.gcpError })}
        </p>
      )}
      {s.gcpChecking ? (
        <CheckingNote>{t("servers.setup.credentials.checkingGcloud")}</CheckingNote>
      ) : s.gcpEmail ? (
        <>
          {/* The sign-in, its log, then the billing account it gives access to. */}
          <SignedInNote s={s}>
            <span className="min-w-0 truncate">
              <span className="text-muted-foreground">{t("servers.setup.credentials.signedInAs")} </span>
              <span className="text-foreground">{s.gcpEmail}</span>
            </span>
          </SignedInNote>
          <SignInLog s={s} />
          {gcpBilling.length > 0 ? (
            <Field label={t("servers.setup.credentials.billingAccount")} hint={t("servers.setup.credentials.billingHint")}>
              <Select value={v.username} onChange={(e) => s.set("username", e.target.value)}>
                {!v.username && <option value="">{t("servers.setup.credentials.chooseBilling")}</option>}
                {gcpBilling.map((b) => (
                  <option key={b.id} value={b.id} disabled={!b.open}>
                    {b.name ? `${b.name} (${b.id})` : b.id}
                    {b.open ? "" : ` ${t("servers.setup.credentials.closed")}`}
                  </option>
                ))}
              </Select>
            </Field>
          ) : (
            <Field label={t("servers.setup.credentials.billingAccount")} hint={t("servers.setup.credentials.billingNone")}>
              <Input {...s.text("username")} placeholder="0X0X0X-0X0X0X-0X0X0X" />
            </Field>
          )}
          <Field label={t("servers.setup.credentials.organization")}>
            <Select value={v.node ?? ""} onChange={(e) => s.set("node", e.target.value || null)}>
              <option value="">{t("servers.setup.credentials.noOrganization")}</option>
              {s.gcpOrgs.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.name ? `${o.name} (${o.id})` : o.id}
                </option>
              ))}
            </Select>
          </Field>
        </>
      ) : (
        <>
          <SignInNote
            hint={<span className="text-muted-foreground">{t("servers.setup.credentials.gcpSignInHint")}</span>}
            label={t("servers.setup.credentials.gcpSignIn")}
            onSignIn={s.signIn}
            signingIn={s.signingIn}
          />
          <SignInLog s={s} />
          <Field label={t("servers.setup.credentials.billingAccount")} hint={t("servers.setup.credentials.orType")}>
            <Input {...s.text("username")} placeholder="0X0X0X-0X0X0X-0X0X0X" />
          </Field>
        </>
      )}
    </div>
  );
}

/** AWS through its CLI: the browser sign-in, its log, then the profile (as for GCP and Azure). */
export function AwsCliCredentials({ s }: { s: HostSetup }) {
  const t = useT();
  return (
    <div className="space-y-3">
      <SignInNote
        hint={
          s.checkingId ? (
            <span className="flex items-center gap-1.5 text-muted-foreground">
              <Spinner className="size-3.5" /> {t("servers.setup.credentials.checking")}
            </span>
          ) : s.awsIdentity ? (
            <span className="flex min-w-0 items-center gap-1.5">
              <CheckCircle2 className="size-3.5 shrink-0 text-success" />
              <span className="text-muted-foreground">{t("servers.setup.credentials.signedInAs")}</span>
              <span className="truncate text-foreground">{s.awsIdentity}</span>
            </span>
          ) : (
            <span className="text-muted-foreground">{t("servers.setup.credentials.notSignedInProfile")}</span>
          )
        }
        label={t("servers.setup.credentials.browserSignIn")}
        onSignIn={s.awsSignIn}
        signingIn={s.signingIn}
      />
      <SignInLog s={s} />
      {s.profiles.length > 0 && (
        <Field label={t("servers.setup.credentials.profile")}>
          <Select value={s.v.awsProfile ?? ""} onChange={(e) => s.set("awsProfile", e.target.value || null)}>
            <option value="">default</option>
            {/* "default" is the first option already; the CLI lists it among the others too. */}
            {s.profiles
              .filter((p) => p !== "default")
              .map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
          </Select>
        </Field>
      )}
    </div>
  );
}
