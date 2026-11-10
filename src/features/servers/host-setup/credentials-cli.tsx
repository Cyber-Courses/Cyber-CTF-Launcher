"use client";

import { Field, Input, Select } from "@/features/servers/host-setup/form";
import { CheckingNote, SignInLog, SignInNote, SignedInNote } from "@/features/servers/host-setup/credentials-parts";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

// The clouds that sign in through their own CLI (a browser sign-in): Azure and Google Cloud here,
// AWS with CLI credentials in credentials-aws. Each shows the sign-in, its log, then what the
// account gives access to.

/** Azure: the CLI sign-in, then the subscription it gives access to (or one pasted). */
export function AzureCredentials({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, azureSubs } = s;
  return (
    <div className="space-y-3">
      {s.azureError && !s.azureChecking && (
        <p role="alert" className="text-[0.75rem] break-words text-destructive">
          {t("servers.setup.credentials.azureReadFailed", { error: s.azureError })}
        </p>
      )}
      {s.azureChecking ? (
        <CheckingNote>{t("servers.setup.credentials.checkingAzure")}</CheckingNote>
      ) : azureSubs.length > 0 ? (
        <>
          {/* The sign-in, its log, then the subscription it gives access to (as for GCP). */}
          <SignedInNote s={s}>
            <span className="text-muted-foreground">{t("servers.setup.credentials.signedInAzure")}</span>
          </SignedInNote>
          <SignInLog s={s} onRetry={s.signIn} />
          <Field label={t("servers.setup.credentials.subscription")}>
            <Select
              value={v.username}
              onChange={(e) => {
                s.set("username", e.target.value);
              }}
            >
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
          <SignInLog s={s} onRetry={s.signIn} />
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
        <GcpAccount s={s} email={s.gcpEmail} />
      ) : (
        <>
          <SignInNote
            hint={<span className="text-muted-foreground">{t("servers.setup.credentials.gcpSignInHint")}</span>}
            label={t("servers.setup.credentials.gcpSignIn")}
            onSignIn={s.signIn}
            signingIn={s.signingIn}
          />
          <SignInLog s={s} onRetry={s.signIn} />
          <Field label={t("servers.setup.credentials.billingAccount")} hint={t("servers.setup.credentials.orType")}>
            <Input {...s.text("username")} placeholder="0X0X0X-0X0X0X-0X0X0X" />
          </Field>
        </>
      )}
    </div>
  );
}

/** Signed in to Google Cloud: who as, its log, then the billing account and organization. */
function GcpAccount({ s, email }: { s: HostSetup; email: string }) {
  const t = useT();
  const { v } = s;
  return (
    <>
      <SignedInNote s={s}>
        <span className="min-w-0 truncate">
          <span className="text-muted-foreground">{t("servers.setup.credentials.signedInAs")} </span>
          <span className="text-foreground">{email}</span>
        </span>
      </SignedInNote>
      <SignInLog s={s} onRetry={s.signIn} />
      {s.gcpBilling.length > 0 ? (
        <Field label={t("servers.setup.credentials.billingAccount")} hint={t("servers.setup.credentials.billingHint")}>
          <Select
            value={v.username}
            onChange={(e) => {
              s.set("username", e.target.value);
            }}
          >
            {!v.username && <option value="">{t("servers.setup.credentials.chooseBilling")}</option>}
            {s.gcpBilling.map((b) => (
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
        <Select
          value={v.node ?? ""}
          onChange={(e) => {
            s.set("node", e.target.value || null);
          }}
        >
          <option value="">{t("servers.setup.credentials.noOrganization")}</option>
          {s.gcpOrgs.map((o) => (
            <option key={o.id} value={o.id}>
              {o.name ? `${o.name} (${o.id})` : o.id}
            </option>
          ))}
        </Select>
      </Field>
    </>
  );
}
