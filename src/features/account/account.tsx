"use client";

import { type ReactNode, useState } from "react";
import { Check, Cog, Copy } from "lucide-react";
import { authLogout, type AuthStatus } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { CopyValue, useCopy } from "@/components/ui/copy-value";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { useNow } from "@/lib/use-now";
import { cancelSignIn, signIn, signInWithCode, useSignIn } from "@/features/account/sign-in";
import { cn } from "@/lib/utils";

export function initials(name: string | null, email: string | null): string {
  const src = (name || email || "").trim();
  if (!src) return "?";
  const parts = src.split(/\s+/);
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
  return src.slice(0, 2).toUpperCase();
}

/** The account's picture, its initials until it loads or when there is none (or it fails to). */
export function Avatar({ status, className, children }: { status: Pick<AuthStatus, "name" | "email" | "picture">; className?: string; children?: ReactNode }) {
  const [broken, setBroken] = useState<string | null>(null);
  const picture = status.picture && status.picture !== broken ? status.picture : null;
  return (
    <span className={cn("relative flex shrink-0", className)}>
      <span className="avatar flex size-full items-center justify-center overflow-hidden rounded-full font-semibold">
        {initials(status.name, status.email)}
        {picture && (
          // eslint-disable-next-line @next/next/no-img-element -- static export, a remote picture
          <img
            src={picture}
            alt=""
            className="absolute inset-0 size-full object-cover"
            onError={() => {
              setBroken(picture);
            }}
          />
        )}
      </span>
      {children}
    </span>
  );
}

/** Signs in through the browser (one sign-in at a time, see sign-in.ts) and reports the new
 *  status; a failure stays in the sign-in's state as `error`, and is shown from there. */
async function signInAndReport(onChange: (status: AuthStatus) => void) {
  try {
    const status = await signIn();
    if (status) onChange(status);
  } catch {
    // Kept in the sign-in's state as `error`.
  }
}

/** Signs out. The local auth state is always cleared, even if the backend's sign-out fails, so
 *  the UI can't get stuck "signed in" with no way out. */
async function signOut(onChange: (status: AuthStatus) => void) {
  try {
    await authLogout();
  } catch (e) {
    // The click handler has no one to reject to: log it rather than leave it unhandled.
    warn("sign out")(e);
  } finally {
    onChange({ loggedIn: false, name: null, email: null });
  }
}

/** Shared sign-in/out actions, so the sidebar, Settings and the onboarding drive the same auth
 *  state and the same sign-in (features/account/sign-in): one at a time, its wait and failure shown in each. */
export function useAuthActions(onChange: (status: AuthStatus) => void) {
  const { waiting, error } = useSignIn();
  const login = () => {
    void signInAndReport(onChange);
  };
  const logout = () => {
    void signOut(onChange);
  };
  return { login, logout, busy: waiting, error };
}

/** Why the player is signed out when it wasn't their doing: the session expired (said until
 *  they sign in or out), or the keychain couldn't be read. */
export function signedOutReason(t: ReturnType<typeof useT>, status: AuthStatus): string | null {
  if (status.loggedIn) return null;
  if (status.keychainError) return status.keychainError;
  return status.sessionExpired ? t("account.sessionExpired") : null;
}

/** `m:ss` left until `at`, never below zero. */
function timeLeft(at: number, now: number): string {
  const secs = Math.max(0, Math.round((at - now) / 1000));
  return `${String(Math.floor(secs / 60))}:${String(secs % 60).padStart(2, "0")}`;
}

/** A small action that copies `text`, and shows a check for a moment once it did. */
function CopyAction({ text, label }: { text: string; label: string }) {
  const t = useT();
  const { copied, copy } = useCopy(text, { showFor: 1200, failure: t("errors.clipboard") });
  return (
    <Button variant="ghost" size="xs" onClick={copy}>
      {copied ? <Check className="size-3.5 text-success" /> : <Copy className="size-3.5" />} {label}
    </Button>
  );
}

/**
 * The sign-in while it waits, in place of the Sign in button: what to do (finish in the browser,
 * or enter the code on any device, with how long the code lasts), and the ways out. The sign-in
 * link can be copied into a browser when none showed it (a broken default browser accepts the
 * link and does nothing), the sign-in switched to a code (a machine whose browser can't be
 * used), or cancelled. The ways out show once the link or the code came: the core's wait is
 * registered by then, so Cancel always reaches it.
 */
export function SignInWait({ className }: { className?: string }) {
  const t = useT();
  const { waiting, url, code, codeExpiresAt } = useSignIn();
  const [now] = useNow(1000, code !== null);
  if (!waiting) return null;
  const cancel = (
    <Button variant="ghost" size="xs" onClick={() => void cancelSignIn().catch(warn("cancel sign-in"))}>
      {t("account.cancel")}
    </Button>
  );
  const title = (text: string) => (
    <p className="flex items-center gap-2 font-medium text-foreground">
      <Spinner className="size-3.5" /> {text}
    </p>
  );
  if (code) {
    const page = code.verificationUriComplete ?? code.verificationUri;
    const left = codeExpiresAt === null ? null : timeLeft(codeExpiresAt, now);
    return (
      <div className={cn("surface-panel rounded-panel px-3 py-2.5 text-[0.75rem] text-muted-foreground", className)}>
        {title(t("account.codeTitle"))}
        <CopyValue text={code.userCode} title={t("account.copyCode")} className="mt-2 font-mono text-[1rem] font-semibold tracking-[0.2em] text-foreground" />
        <p className="mt-1.5">{t("account.enterCode", { url: code.verificationUri })}</p>
        {left !== null && (
          <p className="mt-1 font-mono text-[0.6875rem] text-faint">{left === "0:00" ? t("account.codeExpired") : t("account.codeExpires", { time: left })}</p>
        )}
        <div className="-ml-2 mt-1.5 flex flex-wrap items-center gap-x-1">
          <CopyAction text={page} label={t("account.copyCodeLink")} />
          {cancel}
        </div>
      </div>
    );
  }
  return (
    <div className={cn("surface-panel rounded-panel px-3 py-2.5 text-[0.75rem] text-muted-foreground", className)}>
      {title(t("account.browserTitle"))}
      <p className="mt-1">{t("account.browserBody")}</p>
      {url && (
        <div className="-ml-2 mt-1.5 flex flex-wrap items-center gap-x-1 gap-y-0.5">
          <CopyAction text={url} label={t("account.copyLink")} />
          {/* Whoever started the browser sign-in gets this one's outcome, and a failure stays in
              the sign-in's state as `error`. */}
          <Button variant="ghost" size="xs" onClick={() => void signInWithCode().catch(warn("sign in with a code"))}>
            {t("account.useCode")}
          </Button>
          {cancel}
        </div>
      )}
    </div>
  );
}

/** Signed out: why, when it wasn't the player's doing; Sign in (or the wait, while it runs); Settings. */
function SignedOut({ status, onSettings, onChange }: { status: AuthStatus; onSettings?: () => void; onChange: (status: AuthStatus) => void }) {
  const t = useT();
  const { login, busy, error } = useAuthActions(onChange);
  const reason = error ?? signedOutReason(t, status);
  return (
    <div className="space-y-2">
      {reason && <p className={cn("text-[0.75rem]", error ? "text-destructive" : "text-warning")}>{reason}</p>}
      {busy ? (
        <SignInWait />
      ) : (
        <Button size="sm" className="w-full" onClick={login}>
          {t("account.signIn")}
        </Button>
      )}
      {onSettings && (
        <button
          onClick={onSettings}
          className="flex w-full items-center justify-center gap-1.5 text-[0.75rem] text-muted-foreground transition-colors hover:text-foreground"
        >
          <Cog className="size-3.5" /> {t("account.settings")}
        </button>
      )}
    </div>
  );
}

export function Account({
  status,
  onChange,
  online,
  onSettings,
}: {
  status: AuthStatus | null;
  onChange: (status: AuthStatus) => void;
  /** Show an online dot on the avatar (signed in means labs from the website run on this machine). */
  online?: boolean;
  /** When set, a Settings gear sits next to Sign out, so the footer is one row. */
  onSettings?: () => void;
}) {
  const t = useT();
  const { logout } = useAuthActions(onChange);
  const iconBtn = "grid size-7 shrink-0 place-items-center rounded-xs text-faint transition-colors hover:bg-glass hover:text-foreground";

  if (!status) return null;
  if (!status.loggedIn) return <SignedOut status={status} onSettings={onSettings} onChange={onChange} />;

  return (
    <div className="flex items-center gap-2.5 px-1">
      <Avatar status={status} className="size-7 text-[0.6875rem]">
        {online && <span title={t("account.online")} className="absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full border-2 border-card bg-success" />}
      </Avatar>
      <div className="min-w-0 flex-1 leading-tight">
        <p className="truncate text-[0.8125rem] font-medium text-foreground">{status.name ?? t("account.signedIn")}</p>
        <p className="truncate font-mono text-[0.625rem] text-faint">{status.email ?? t("account.signedInKeychain")}</p>
      </div>
      {onSettings && (
        <button onClick={onSettings} title={t("account.settings")} aria-label={t("account.settings")} className={iconBtn}>
          <Cog className="size-4" />
        </button>
      )}
      <button onClick={logout} title={t("account.signOut")} aria-label={t("account.signOut")} className={iconBtn}>
        <Icon name="logout" className="size-4" />
      </button>
    </div>
  );
}
