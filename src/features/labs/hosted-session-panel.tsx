"use client";

import { ExternalLink, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Panel } from "@/components/ui/panel";
import { StatusDot } from "@/components/ui/status-pill";
import type { HostedSession } from "@/features/hosted/use-hosted-labs";
import { openExternal } from "@/lib/failure";
import { useT, type T } from "@/lib/i18n";
import { hoursMinutes } from "@/features/labs/lab-timers";

const STAGES = ["REQUESTED", "CLAIMED", "PULLING"] as const;
const stage = (t: T, state: string | undefined) =>
  STAGES.includes(state as (typeof STAGES)[number]) ? t(`labs.hosted.stages.${state as (typeof STAGES)[number]}`) : undefined;

function expiresIn(t: T, iso: string): string {
  const ms = new Date(iso).getTime() - Date.now();
  if (!Number.isFinite(ms) || ms <= 0) return "";
  const mins = Math.floor(ms / 60000);
  const h = Math.floor(mins / 60);
  return t("labs.hosted.expiresIn", { left: hoursMinutes(t, h, mins % 60) });
}

/** Whether a session is over or broken: its button dismisses it rather than stopping it. */
const ended = (state: string | undefined) => state === "FAILED" || state === "STOPPED" || state === "EXPIRED";

/** A lab running hosted by Cyber CTF, on its lab page: starting, running (open its URLs), or failed. */
export function HostedSessionPanel({
  session,
  starting,
  error,
  onStop,
}: {
  session: HostedSession | null;
  starting: boolean;
  error: string | null;
  onStop: () => void;
}) {
  const t = useT();
  const state = session?.state;
  return (
    <Panel>
      <div className="flex flex-wrap items-center gap-3 px-4 py-3">
        <div className="min-w-0 flex-1">
          <SessionState session={session} starting={starting} error={error} />
        </div>
        {(session || error) && (
          <div className="flex flex-wrap items-center gap-2">
            {session && state === "RUNNING" && <SessionLinks session={session} />}
            <Button variant="outline" size="sm" onClick={onStop}>
              {!session || ended(state) ? (
                t("labs.hosted.dismiss")
              ) : (
                <>
                  <Square className="size-3.5" /> {t("labs.hosted.stop")}
                </>
              )}
            </Button>
          </div>
        )}
      </div>
    </Panel>
  );
}

/** Where the session stands: running (and when it expires), failed or slow, over, or waiting. */
function SessionState({ session, starting, error }: { session: HostedSession | null; starting: boolean; error: string | null }) {
  const t = useT();
  const state = session?.state;
  if (session && state === "RUNNING")
    return (
      <>
        <p className="flex items-center gap-2 text-[0.8125rem] font-medium text-foreground">
          <StatusDot tone="ok" /> {t("labs.hosted.running")}
        </p>
        <p className="mt-0.5 pl-4 text-[0.75rem] text-muted-foreground">
          {t("labs.hosted.runningHint")} {expiresIn(t, session.expiresAt)}
        </p>
      </>
    );
  if (state === "FAILED" || error)
    return (
      <>
        <p className="flex items-center gap-2 text-[0.8125rem] font-medium text-foreground">
          <StatusDot tone={state === "FAILED" ? "fail" : "warn"} />
          {state === "FAILED" ? t("labs.hosted.failed") : t("labs.hosted.slow")}
        </p>
        <p className="mt-0.5 pl-4 text-[0.75rem] text-muted-foreground">{error ?? session?.message ?? t("labs.hosted.tryAgain")}</p>
      </>
    );
  if (state === "STOPPED" || state === "EXPIRED")
    return (
      <p className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
        <StatusDot tone="muted" />
        {state === "EXPIRED" ? t("labs.hosted.expired") : t("labs.hosted.ended")}
      </p>
    );
  return (
    <p className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
      <StatusDot tone="warn" pulse />{" "}
      {t("labs.hosted.waiting", { stage: stage(t, state) ?? (starting ? t("labs.hosted.stages.REQUESTED") : t("labs.hosted.starting")) })}
    </p>
  );
}

/** A running session's URLs: one Open button per published port. */
function SessionLinks({ session }: { session: HostedSession }) {
  const t = useT();
  return session.endpoints.map((e) => (
    <Button
      key={e.port}
      variant="outline"
      size="sm"
      onClick={() => {
        openExternal(e.url);
      }}
    >
      {session.endpoints.length > 1 ? t("labs.hosted.openPort", { port: e.port }) : t("labs.hosted.open")} <ExternalLink className="size-3.5" />
    </Button>
  ));
}
