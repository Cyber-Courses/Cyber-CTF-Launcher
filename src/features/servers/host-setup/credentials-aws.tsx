"use client";

import { AlertTriangle, CheckCircle2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Field, Note, Select } from "@/features/servers/host-setup/form";
import { ExternalLinkButton, SignInLog, SignInNote } from "@/features/servers/host-setup/credentials-parts";
import { AWS_CLI_UPDATE_URL, awsCliTooOld } from "@/features/servers/host-setup/setup-model";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

/** AWS through its CLI: the browser sign-in (`aws login`), its log, then the profile (as for GCP
 *  and Azure). An AWS CLI too old for `aws login` says so and where to update it, instead. */
export function AwsCliCredentials({ s }: { s: HostSetup }) {
  const t = useT();
  const tooOld = awsCliTooOld(s.report?.cloudClis.aws.version);
  return (
    <div className="space-y-3">
      {tooOld ? (
        <AwsCliTooOld s={s} version={tooOld} />
      ) : (
        <SignInNote hint={<AwsIdentity s={s} />} label={t("servers.setup.credentials.browserSignIn")} onSignIn={s.awsSignIn} signingIn={s.signingIn} />
      )}
      <SignInLog s={s} onRetry={s.awsSignIn} />
      {s.profiles.length > 0 && (
        <Field label={t("servers.setup.credentials.profile")}>
          <Select
            value={s.v.awsProfile ?? ""}
            onChange={(e) => {
              s.set("awsProfile", e.target.value || null);
            }}
          >
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

/** Who the chosen AWS profile is signed in as: being checked, the identity, or nobody. */
function AwsIdentity({ s }: { s: HostSetup }) {
  const t = useT();
  if (s.checkingId)
    return (
      <span className="flex items-center gap-1.5 text-muted-foreground">
        <Spinner className="size-3.5" /> {t("servers.setup.credentials.checking")}
      </span>
    );
  if (s.awsIdentity)
    return (
      <span className="flex min-w-0 items-center gap-1.5">
        <CheckCircle2 className="size-3.5 shrink-0 text-success" />
        <span className="text-muted-foreground">{t("servers.setup.credentials.signedInAs")}</span>
        <span className="truncate text-foreground">{s.awsIdentity}</span>
      </span>
    );
  return <span className="text-muted-foreground">{t("servers.setup.credentials.notSignedInProfile")}</span>;
}

/** An AWS CLI without `aws login` (before 2.32): which version it is, where to update it, and a
 *  check again once it is; a profile already signed in still works meanwhile. */
function AwsCliTooOld({ s, version }: { s: HostSetup; version: string }) {
  const t = useT();
  return (
    <Note role="status" className="space-y-2">
      <p className="flex items-start gap-2">
        <AlertTriangle className="mt-px size-3.5 shrink-0 text-warning" />
        <span className="text-muted-foreground">{t("servers.setup.credentials.awsCliTooOld", { version })}</span>
      </p>
      <p className="flex flex-wrap items-center gap-3 pl-5.5">
        <ExternalLinkButton url={AWS_CLI_UPDATE_URL} className="text-foreground">
          {t("servers.setup.credentials.awsCliUpdate")}
        </ExternalLinkButton>
        <Button variant="outline" size="xs" onClick={s.onRefresh}>
          {t("servers.setup.nav.recheck")}
        </Button>
        <span className="text-muted-foreground">
          <AwsIdentity s={s} />
        </span>
      </p>
    </Note>
  );
}
