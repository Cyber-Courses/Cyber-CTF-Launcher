"use client";

import { useEffect, useRef, useState } from "react";
import { Cloud } from "lucide-react";
import { Button } from "@/components/ui/button";
import { confirmLaunch } from "@/lib/tauri";
import { useFocusTrap } from "@/lib/use-focus-trap";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

type Request = { sessionId: string; repository: string; commit: string; target: string };

/** How long the agent waits for an answer (account/agent.rs: 180 s), less a margin so an answer
 *  given at the very end isn't lost on its way: past it, Approve would silently do nothing. */
export const CONFIRM_WINDOW_MS = 175_000;

/**
 * A website-launched lab that would run on one of the player's cloud accounts (which costs
 * money) isn't started unattended: the agent emits `launch-confirm` and waits here for the
 * player to approve or decline on this machine.
 */
export function LaunchConfirm() {
  const t = useT();
  const [req, setReq] = useState<Request | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  // The request whose answer window ran out: it can only be dismissed.
  const [expired, setExpired] = useState<string | null>(null);

  useTauriEvent<Request>("launch-confirm", setReq);
  useFocusTrap(ref, !!req);
  useEffect(() => {
    if (!req) return;
    const timer = setTimeout(() => setExpired(req.sessionId), CONFIRM_WINDOW_MS);
    return () => clearTimeout(timer);
  }, [req]);

  if (!req) return null;
  const isExpired = expired === req.sessionId;

  const answer = (approve: boolean) => {
    setBusy(true);
    void confirmLaunch(req.sessionId, approve)
      .catch(tell(t("app.launchConfirm.failed")))
      .finally(() => {
        setBusy(false);
        setReq(null);
      });
  };

  return (
    <div className="animate-fade-in fixed inset-0 z-[60] flex items-center justify-center bg-[var(--scrim)] p-4 backdrop-blur-[0.125rem]">
      <div
        ref={ref}
        tabIndex={-1}
        role="alertdialog"
        aria-modal="true"
        aria-label={t("app.launchConfirm.label")}
        className="w-full max-w-[28rem] surface-glass rounded-[1rem] p-5 outline-none"
      >
        <div className="flex items-start gap-3">
          <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-control bg-jewel/10 text-jewel-text shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--jewel)_30%,transparent)]">
            <Cloud className="size-4" />
          </span>
          <div className="min-w-0">
            <p className="text-[0.9375rem] font-medium text-foreground">{t("app.launchConfirm.title")}</p>
            <p className="mt-1 text-[0.8125rem] leading-relaxed text-muted-foreground">
              {t.rich("app.launchConfirm.body", { target: (s) => <span className="text-foreground">{s}</span> }, { target: req.target })}
            </p>
            <div className="mt-2.5 space-y-0.5 font-mono text-[0.6875rem] text-faint">
              <p className="truncate">{t("app.launchConfirm.repo", { repo: req.repository })}</p>
              <p className="truncate">{t("app.launchConfirm.commit", { commit: req.commit.slice(0, 12) })}</p>
            </div>
          </div>
        </div>
        {isExpired && (
          <p role="status" className="mt-4 text-[0.8125rem] text-muted-foreground">
            {t("app.launchConfirm.expired")}
          </p>
        )}
        <div className="mt-5 flex justify-end gap-2">
          {isExpired ? (
            <Button size="sm" onClick={() => setReq(null)}>
              {t("app.launchConfirm.close")}
            </Button>
          ) : (
            <>
              <Button variant="ghost" size="sm" disabled={busy} onClick={() => answer(false)}>
                {t("app.launchConfirm.decline")}
              </Button>
              <Button size="sm" disabled={busy} onClick={() => answer(true)}>
                {t("app.launchConfirm.approve")}
              </Button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
