"use client";

import { useEffect, useRef, useState, type ReactNode } from "react";
import { Cloud, Laptop, Server, ShieldAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { confirmLaunch, type LaunchConfirmRequest } from "@/lib/tauri";
import { useFocusTrap } from "@/lib/use-focus-trap";
import { useTauriEvent } from "@/lib/use-tauri-event";
import { tell } from "@/lib/failure";
import { useT } from "@/lib/i18n";

type Request = LaunchConfirmRequest;
type Place = Request["place"];

const PLACES: readonly Place[] = ["here", "server", "cloud"];
const ICONS = { here: Laptop, server: Server, cloud: Cloud } as const;

/** How long the agent waits for an answer (account/agent.rs: 180 s), less a margin so an answer
 *  given at the very end isn't lost on its way: past it, Approve would silently do nothing. */
export const CONFIRM_WINDOW_MS = 175_000;

/** The request in a `launch-confirm` event's payload, or null for anything else. */
function parseRequest(payload: unknown): Request | null {
  if (typeof payload !== "object" || payload === null) return null;
  const { sessionId, labName, repository, commit, target, place, privileged } = payload as Record<string, unknown>;
  const fields = [sessionId, labName, repository, commit, target];
  const known = fields.every((f) => typeof f === "string") && PLACES.includes(place as Place) && typeof privileged === "boolean";
  return known ? (payload as Request) : null;
}

/** The launch waiting for the player's answer, whether its answer window ran out, and the
 *  answer on its way. */
function useLaunchRequest() {
  const t = useT();
  const [req, setReq] = useState<Request | null>(null);
  const [busy, setBusy] = useState(false);
  // The request whose answer window ran out: it can only be dismissed.
  const [expired, setExpired] = useState<string | null>(null);

  useTauriEvent("launch-confirm", (payload) => {
    const next = parseRequest(payload);
    if (next) setReq(next);
  });
  useEffect(() => {
    if (!req) return;
    const timer = setTimeout(() => {
      setExpired(req.sessionId);
    }, CONFIRM_WINDOW_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [req]);

  const answer = (approve: boolean) => {
    if (!req) return;
    setBusy(true);
    void confirmLaunch(req.sessionId, approve)
      .catch(tell(t("app.launchConfirm.failed")))
      .finally(() => {
        setBusy(false);
        setReq(null);
      });
  };
  const dismiss = () => {
    setReq(null);
  };
  return { req, busy, isExpired: req !== null && expired === req.sessionId, answer, dismiss };
}

/**
 * A lab launched from the website never starts unattended, wherever it would run (this computer,
 * a server, a cloud account, which costs money): the agent emits `launch-confirm` and waits here
 * for the player to approve or decline on this machine. A lab with privileged machines says so.
 */
export function LaunchConfirm() {
  const t = useT();
  const ref = useRef<HTMLDivElement>(null);
  const { req, busy, isExpired, answer, dismiss } = useLaunchRequest();
  // Escape declines (nothing starts, nothing is billed), or closes a request that ran out.
  useFocusTrap(ref, !!req, () => {
    if (!req || busy) return;
    if (isExpired) dismiss();
    else answer(false);
  });

  if (!req) return null;

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
        <RequestSummary req={req} />
        {isExpired && (
          <p role="status" className="mt-4 text-[0.8125rem] text-muted-foreground">
            {t("app.launchConfirm.expired")}
          </p>
        )}
        <div className="mt-5 flex justify-end gap-2">
          {isExpired ? (
            <Button size="sm" onClick={dismiss}>
              {t("app.launchConfirm.close")}
            </Button>
          ) : (
            <AnswerButtons place={req.place} busy={busy} answer={answer} />
          )}
        </div>
      </div>
    </div>
  );
}

/** What would start, and where. */
function RequestSummary({ req }: { req: Request }) {
  const t = useT();
  const Icon = ICONS[req.place];
  const strong = (s: ReactNode) => <span className="text-foreground">{s}</span>;
  return (
    <div className="flex items-start gap-3">
      <span className="mt-0.5 grid size-8 shrink-0 place-items-center rounded-control bg-jewel/10 text-jewel-text shadow-[inset_0_0_0_1px_color-mix(in_oklab,var(--jewel)_30%,transparent)]">
        <Icon className="size-4" />
      </span>
      <div className="min-w-0">
        <p className="text-[0.9375rem] font-medium text-foreground">{t(`app.launchConfirm.title.${req.place}`)}</p>
        <p className="mt-1 text-[0.8125rem] leading-relaxed text-muted-foreground">
          {t.rich(`app.launchConfirm.body.${req.place}`, { lab: strong, target: strong }, { lab: req.labName, target: req.target })}
        </p>
        {req.privileged && (
          <p className="mt-2.5 flex items-start gap-1.5 text-[0.8125rem] leading-relaxed text-warning">
            <ShieldAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden />
            {t("app.launchConfirm.privileged")}
          </p>
        )}
        <div className="mt-2.5 space-y-0.5 font-mono text-[0.6875rem] text-faint">
          <p className="truncate">{t("app.launchConfirm.repo", { repo: req.repository })}</p>
          <p className="truncate">{t("app.launchConfirm.commit", { commit: req.commit.slice(0, 12) })}</p>
        </div>
      </div>
    </div>
  );
}

/** Decline or approve, while no answer is on its way. */
function AnswerButtons({ place, busy, answer }: { place: Place; busy: boolean; answer: (approve: boolean) => void }) {
  const t = useT();
  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        disabled={busy}
        onClick={() => {
          answer(false);
        }}
      >
        {t("app.launchConfirm.decline")}
      </Button>
      <Button
        size="sm"
        disabled={busy}
        onClick={() => {
          answer(true);
        }}
      >
        {t(`app.launchConfirm.approve.${place}`)}
      </Button>
    </>
  );
}
