"use client";

import { LogOut } from "lucide-react";
import { initials, SignInHelp, useAuthActions } from "@/features/account/account";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { StatusPill } from "@/components/ui/status-pill";
import { type AgentInfo, type AuthStatus } from "@/lib/tauri";
import { Row, Section } from "@/features/settings/settings-layout";
import { useT } from "@/lib/i18n";

/* ------------------------------------------------------------------ account */

export function AccountSection({ auth, agent, onAuthChange }: { auth: AuthStatus | null; agent: AgentInfo | null; onAuthChange: (s: AuthStatus) => void }) {
  const t = useT();
  return (
    <Section title={t("settings.account.title")} description={t("settings.account.description")}>
      {auth === null ? (
        <Row
          title={
            <span className="flex items-center gap-2 text-muted-foreground">
              <Spinner className="size-3.5" /> {t("settings.account.checking")}
            </span>
          }
        />
      ) : auth.loggedIn ? (
        <SignedIn auth={auth} agent={agent} onAuthChange={onAuthChange} />
      ) : (
        <SignedOut auth={auth} onAuthChange={onAuthChange} />
      )}
    </Section>
  );
}

/** The player, Sign out, and this machine as the website sees it. */
function SignedIn({ auth, agent, onAuthChange }: { auth: AuthStatus; agent: AgentInfo | null; onAuthChange: (s: AuthStatus) => void }) {
  const t = useT();
  const { logout } = useAuthActions(onAuthChange);
  return (
    <>
      <div className="flex items-center gap-3.5 px-4 py-3.5">
        <span className="avatar flex size-10 shrink-0 items-center justify-center rounded-full text-[0.8125rem] font-semibold">
          {initials(auth.name, auth.email)}
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[0.8125rem] font-medium text-foreground">{auth.name ?? t("settings.account.signedIn")}</p>
          {auth.email && <p className="truncate font-mono text-[0.6875rem] text-faint">{auth.email}</p>}
        </div>
        <Button variant="outline" size="xs" onClick={logout}>
          <LogOut className="size-3.5" /> {t("settings.account.signOut")}
        </Button>
      </div>
      <Row
        title={
          <span className="flex items-center gap-2">
            {t("settings.account.thisMachine")}
            <StatusPill tone="ok">{t("settings.account.online")}</StatusPill>
          </span>
        }
        description={agent ? <AgentLine agent={agent} /> : t("settings.account.registering")}
      />
    </>
  );
}

/** This machine's agent: its name, architecture and what it runs. */
function AgentLine({ agent }: { agent: AgentInfo }) {
  const t = useT();
  return (
    <>
      {agent.name} <span className="text-faint">·</span> <span className="font-mono text-[0.6875rem] text-faint">{agent.arch}</span>
      {agent.capabilities.length > 0 && (
        <>
          {" "}
          <span className="text-faint">·</span> {t("settings.account.runs", { list: agent.capabilities.join(", ") })}
        </>
      )}
    </>
  );
}

/** Not signed in: why (a failed sign-in, the keychain), and Sign in with its wait. */
function SignedOut({ auth, onAuthChange }: { auth: AuthStatus; onAuthChange: (s: AuthStatus) => void }) {
  const t = useT();
  const { login, busy, error } = useAuthActions(onAuthChange);
  const problem = error ?? auth.keychainError;
  return (
    <Row
      title={
        <span className="flex items-center gap-2">
          {t("settings.account.notSignedIn")}
          <StatusPill tone="muted">{t("settings.account.offline")}</StatusPill>
        </span>
      }
      description={
        <>
          {problem ? <span className="text-destructive">{problem}</span> : t("settings.account.signInHint")}
          <SignInHelp className="mt-1.5 justify-start" />
        </>
      }
      control={
        <Button size="sm" onClick={login} disabled={busy}>
          {busy ? (
            <>
              <Spinner className="size-3.5" /> {t("settings.account.waitingBrowser")}
            </>
          ) : (
            t("settings.account.signIn")
          )}
        </Button>
      }
    />
  );
}
