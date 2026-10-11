"use client";

import { useState } from "react";
import { Cog } from "lucide-react";
import { authLogin, authLogout, type AuthStatus } from "@/lib/tauri";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";

export function initials(name: string | null, email: string | null): string {
  const src = (name || email || "").trim();
  if (!src) return "?";
  const parts = src.split(/\s+/);
  if (parts.length >= 2) return (parts[0][0] + parts[1][0]).toUpperCase();
  return src.slice(0, 2).toUpperCase();
}

/** Shared sign-in/out actions, so the sidebar and Settings drive the same auth state. */
export function useAuthActions(onChange: (status: AuthStatus) => void) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function login() {
    setBusy(true);
    setError(null);
    try {
      onChange(await authLogin());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
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

  return { login, logout, busy, error };
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
        <button onClick={onSettings} title={t("account.settings")} className={iconBtn}>
          <Cog className="size-4" />
        </button>
      )}
      <button onClick={logout} title={t("account.signOut")} className={iconBtn}>
        <Icon name="logout" className="size-4" />
      </button>
    </div>
  );
}
