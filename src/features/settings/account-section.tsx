"use client";

import { LogOut } from "lucide-react";
import { Avatar, signedOutReason, SignInWait, useAuthActions } from "@/features/account/account";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { StatusPill } from "@/components/ui/status-pill";
import { type AgentInfo, type AgentState, type AuthStatus } from "@/lib/tauri";
import { Row, Section } from "@/features/settings/settings-layout";
import { describeError } from "@/lib/errors";
import { useT, type MessageKey } from "@/lib/i18n";

/* ------------------------------------------------------------------ account */

export function AccountSection({
  auth,
  agent,
  agentError,
  onAuthChange,
}: {
  auth: AuthStatus | null;
  /** This machine's agent; null until it is read. */
  agent: AgentInfo | null;
  /** Why the last read of the agent failed, if it did. */
  agentError?: unknown;
  onAuthChange: (s: AuthStatus) => void;
}) {
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
        <SignedIn auth={auth} agent={agent} agentError={agentError} onAuthChange={onAuthChange} />
      ) : (
        <SignedOut auth={auth} onAuthChange={onAuthChange} />
      )}
    </Section>
  );
}

/** The player, Sign out, and this machine as the website sees it. */
function SignedIn({
  auth,
  agent,
  agentError,
  onAuthChange,
}: {
  auth: AuthStatus;
  agent: AgentInfo | null;
  agentError: unknown;
  onAuthChange: (s: AuthStatus) => void;
}) {
  const t = useT();
  const { logout } = useAuthActions(onAuthChange);
  return (
    <>
      <div className="flex items-center gap-3.5 px-4 py-3.5">
        <Avatar status={auth} className="size-10 text-[0.8125rem]" />
        <div className="min-w-0 flex-1">
          <p className="truncate text-[0.8125rem] font-medium text-foreground">{auth.name ?? t("settings.account.signedIn")}</p>
          {auth.email && <p className="truncate font-mono text-[0.6875rem] text-faint">{auth.email}</p>}
        </div>
        <Button variant="outline" size="xs" onClick={logout}>
          <LogOut className="size-3.5" /> {t("settings.account.signOut")}
        </Button>
      </div>
      <ThisMachine agent={agent} error={agentError} />
    </>
  );
}

/** How each state of the agent reads: its pill, and what is said of it under the machine. */
const AGENT_STATES = {
  online: { tone: "ok", pill: "settings.account.online", note: null },
  registering: { tone: "muted", pill: "settings.account.registering", note: null },
  offByPolicy: { tone: "muted", pill: "settings.account.offline", note: "settings.account.offByPolicy" },
} as const satisfies Record<AgentState, { tone: "ok" | "muted"; pill: MessageKey; note: MessageKey | null }>;

/** This machine as the website sees it: online, still registering, or off by the policy; while
 *  it is read, saying so, and why a read failed. */
function ThisMachine({ agent, error }: { agent: AgentInfo | null; error: unknown }) {
  const t = useT();
  const state = agent ? AGENT_STATES[agent.state] : null;
  return (
    <Row
      title={
        <span className="flex items-center gap-2">
          {t("settings.account.thisMachine")}
          {state && <StatusPill tone={state.tone}>{t(state.pill)}</StatusPill>}
        </span>
      }
      description={
        <>
          {agent ? <AgentLine agent={agent} /> : error === undefined && t("settings.account.checking")}
          {state?.note && <span className="mt-1 block">{t(state.note)}</span>}
          {error !== undefined && <span className="block text-destructive">{t("settings.account.agentFailed", { error: describeError(error, t) })}</span>}
        </>
      }
    />
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

/** Not signed in: why (a failed sign-in, an expired session, the keychain), and Sign in, or
 *  the sign-in's wait while it runs. */
function SignedOut({ auth, onAuthChange }: { auth: AuthStatus; onAuthChange: (s: AuthStatus) => void }) {
  const t = useT();
  const { login, busy, error } = useAuthActions(onAuthChange);
  const problem = error ?? signedOutReason(t, auth);
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
          {problem ? <span className={error ? "text-destructive" : "text-warning"}>{problem}</span> : t("settings.account.signInHint")}
          {busy && <SignInWait className="mt-2.5 max-w-[26rem]" />}
        </>
      }
      control={
        busy ? undefined : (
          <Button size="sm" onClick={login}>
            {t("settings.account.signIn")}
          </Button>
        )
      }
    />
  );
}
