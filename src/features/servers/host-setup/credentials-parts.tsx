"use client";

import type { ReactNode } from "react";
import { CheckCircle2, CircleSlash, ExternalLink, RotateCcw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { LogConsole } from "@/components/ui/log-console";
import { Input, Note } from "@/features/servers/host-setup/form";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { openExternal } from "@/lib/failure";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

// The pieces the cloud credentials step repeats per provider.

/** An underlined link that opens in the player's browser. */
export function ExternalLinkButton({ url, className, children }: { url: string; className?: string; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={() => {
        openExternal(url);
      }}
      className={cn("inline-flex items-center gap-0.5 underline-offset-2 hover:underline", className)}
    >
      {children} <ExternalLink className="size-3" />
    </button>
  );
}

/** A CLI read in progress ("Checking your Azure sign-in…"). */
export function CheckingNote({ children }: { children: ReactNode }) {
  return (
    <Note className="flex items-center gap-2 text-muted-foreground">
      <Spinner className="size-3.5" /> {children}
    </Note>
  );
}

/** Signed in through the CLI: who as, and a link to sign in as someone else. */
export function SignedInNote({ s, children }: { s: HostSetup; children: ReactNode }) {
  const t = useT();
  return (
    <Note className="flex items-center gap-2">
      <CheckCircle2 className="size-3.5 shrink-0 text-success" />
      {children}
      <button
        type="button"
        onClick={s.signIn}
        disabled={s.signingIn}
        className="ml-auto shrink-0 text-[0.6875rem] text-muted-foreground underline-offset-2 hover:text-foreground hover:underline disabled:opacity-50"
      >
        {s.signingIn ? t("servers.setup.credentials.signingIn") : t("servers.setup.credentials.switchAccount")}
      </button>
    </Note>
  );
}

/** Not signed in yet: why it's needed, and the sign-in button. */
export function SignInNote({ hint, label, onSignIn, signingIn }: { hint: ReactNode; label: string; onSignIn: () => void; signingIn: boolean }) {
  return (
    <Note className="flex flex-wrap items-center gap-2">
      {hint}
      <Button variant="outline" size="xs" className="ml-auto" onClick={onSignIn} disabled={signingIn}>
        {signingIn ? <Spinner className="size-3" /> : null} {label}
      </Button>
    </Note>
  );
}

/** The sign-in's streamed log, once one ran, and a way out while it waits for the browser; the
 *  same for every cloud. Cancelled, the log goes: the step says so and offers `onRetry` (the
 *  cloud's own sign-in). A failure keeps its log, ending on why. */
export function SignInLog({ s, onRetry }: { s: HostSetup; onRetry: () => void }) {
  const t = useT();
  if (s.signInState === "cancelled")
    return (
      <Note role="status" className="flex flex-wrap items-center gap-2">
        <CircleSlash className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="text-muted-foreground">{t("servers.setup.credentials.signInCancelled")}</span>
        <Button variant="outline" size="xs" className="ml-auto" onClick={onRetry}>
          <RotateCcw className="size-3" /> {t("servers.setup.credentials.retrySignIn")}
        </Button>
      </Note>
    );
  if (!s.signInLog) return null;
  return (
    <>
      <LogConsole lines={s.signInLog} running={s.signingIn} title={t("servers.setup.credentials.signInLog")} collapseOnDone />
      {s.signingIn && (
        <Button variant="ghost" size="xs" onClick={s.cancelSignIn}>
          {t("servers.setup.credentials.cancelSignIn")}
        </Button>
      )}
    </>
  );
}

/** The host's or account's secret (a password, a token, a secret key), kept in the OS keychain.
 *  Left empty while editing, the saved one stays. */
export function SecretInput({ s, placeholder = "" }: { s: HostSetup; placeholder?: string }) {
  const t = useT();
  return (
    <Input
      type="password"
      value={s.v.password ?? ""}
      onChange={(e) => {
        s.set("password", e.target.value || null);
      }}
      placeholder={s.secretKept ? t("servers.setup.connection.keepPlaceholder") : placeholder}
      autoComplete="off"
    />
  );
}
