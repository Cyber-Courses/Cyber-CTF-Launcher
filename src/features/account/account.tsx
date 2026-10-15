"use client";

import { Cog } from "lucide-react";
import { authLogout, type AuthStatus } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { CopyValue } from "@/components/ui/copy-value";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";
import { cancelSignIn, signIn, useSignIn } from "@/features/account/sign-in";
import { cn } from "@/lib/utils";

export function initials(name: string | null, email: string | null): string {
  const src = (name || email || "").trim();
  if (!src) return "?";
  const parts = src.split(/\s+/);
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
  return src.slice(0, 2).toUpperCase();
}

/** Shared sign-in/out actions, so the sidebar, Settings and the onboarding drive the same auth
 *  state and the same sign-in (features/account/sign-in): one at a time, its wait and failure shown in each. */
export function useAuthActions(onChange: (status: AuthStatus) => void) {
  const { waiting, error } = useSignIn();

  async function login() {
    try {
      const status = await signIn();
      if (status) onChange(status);
    } catch {
      // Kept in the sign-in's state as `error`, and shown from there.
    }
  }

  async function logout() {
    // Always clear local auth state, even if the backend logout call fails, so the UI can't get
    // stuck "signed in" with no way out.
    try {
      await authLogout();
    } catch (e) {
      // The click handler has no one to reject to: log it rather than leave it unhandled.
      warn("sign out")(e);
    } finally {
      onChange({ loggedIn: false, name: null, email: null });
    }
  }

  return { login, logout, busy: waiting, error };
}

/** Under a sign-in's spinner: the sign-in link to copy into a browser when none showed it (a
 *  broken default browser accepts the link and does nothing), and Cancel. */
export function SignInHelp({ className }: { className?: string }) {
  const t = useT();
  const { waiting, url } = useSignIn();
  // Once the link came: the backend's wait is registered by then, so Cancel always reaches it.
  if (!waiting || !url) return null;
  return (
    <div className={cn("flex flex-wrap items-center justify-center gap-x-2 gap-y-1 text-[0.75rem] text-muted-foreground", className)}>
      <span className="flex items-center gap-1.5">
        {t("account.didntOpen")} <CopyValue text={url} label={t("account.copyLink")} title={t("account.copyLink")} />
      </span>
      <Button variant="ghost" size="xs" onClick={() => void cancelSignIn().catch(warn("cancel sign-in"))}>
        {t("account.cancel")}
      </Button>
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
  const { login, logout, busy, error } = useAuthActions(onChange);
  const iconBtn = "grid size-7 shrink-0 place-items-center rounded-xs text-faint transition-colors hover:bg-glass hover:text-foreground";

  if (!status) return null;

  if (!status.loggedIn) {
    return (
      <div className="space-y-2">
        {error && <p className="text-[0.75rem] text-destructive">{error}</p>}
        <Button size="sm" className="w-full" onClick={login} disabled={busy}>
          {busy ? (
            <>
              <Spinner className="size-3.5" /> {t("account.waitingForBrowser")}
            </>
          ) : (
            t("account.signIn")
          )}
        </Button>
        <SignInHelp />
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

  return (
    <div className="flex items-center gap-2.5 px-1">
      <span className="avatar relative flex size-7 shrink-0 items-center justify-center rounded-full text-[0.6875rem] font-semibold">
        {initials(status.name, status.email)}
        {online && <span title={t("account.online")} className="absolute -right-0.5 -bottom-0.5 size-2.5 rounded-full border-2 border-card bg-success" />}
      </span>
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
