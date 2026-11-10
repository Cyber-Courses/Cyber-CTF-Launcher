"use client";

import { useState } from "react";
import { AlertTriangle, CheckCircle2, Play, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Nav, Note, Step } from "@/features/servers/host-setup/form";
import type { ServerCheck } from "@/lib/tauri";
import { outcomeOf, type Outcome } from "@/features/servers/test-outcome";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { ServerSelfTest } from "@/features/servers/server-self-test";
import { useT } from "@/lib/i18n";

export function TestStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { onDone, saved, test, runTest, v } = s;
  // Each part checked (a cloud's checklist, the parts of a Proxmox setup made with an SSH key).
  const parts = test !== null && test !== "testing" ? test.checks : [];
  // Once Proxmox answers as a launch needs, a real throwaway lab proves it can run one: a new
  // one each time the connection is tested again.
  const [round, setRound] = useState(0);
  const vmHost = v.provider === "proxmox" && test !== null && test !== "testing" && test.ok ? saved : null;
  return (
    <Step icon={CheckCircle2} title={t("servers.setup.test.title")} description={t("servers.setup.test.description")}>
      {test === null || test === "testing" ? (
        <p className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
          <Spinner className="size-4" /> {t("servers.setup.test.testing")}
        </p>
      ) : (
        // The result's mark and its message follow one rule (`outcomeOf`, the Cloud page's too):
        // failed on a failing check, whose detail is the message.
        <Note className="flex items-start gap-3 p-3.5">
          <OutcomeGlyph outcome={outcomeOf(test)} className="mt-px size-4" />
          <p className="min-w-0 text-[0.8125rem] break-words text-foreground">
            {test.message}
            {test.latencyMs != null && <span className="ml-1.5 font-mono text-[0.6875rem] text-faint">{test.latencyMs} ms</span>}
          </p>
        </Note>
      )}
      {parts.length > 0 && <Checklist checks={parts} />}
      {vmHost && (
        <div className="mt-4 space-y-2">
          <p className="text-[0.75rem] text-muted-foreground">{t("servers.setup.test.vmLead")}</p>
          <ServerSelfTest key={round} id={vmHost.id} provider="proxmox" onDone={() => {}} />
        </div>
      )}
      <Nav
        left={
          <Button
            variant="outline"
            onClick={() => {
              if (!saved) return;
              setRound((n) => n + 1);
              runTest(saved.id);
            }}
            disabled={test === "testing"}
          >
            {t("servers.setup.nav.testAgain")}
          </Button>
        }
        right={
          <Button onClick={onDone}>
            <Play className="size-4" /> {t("servers.setup.nav.done")}
          </Button>
        }
      />
    </Step>
  );
}

/** Each checked part, with its outcome. */
function Checklist({ checks }: { checks: ServerCheck[] }) {
  const t = useT();
  return (
    <ul className="mt-3 space-y-2" aria-label={t("servers.setup.test.checks")}>
      {checks.map((c) => (
        <li key={c.name} className="flex items-start gap-2.5 text-[0.75rem]">
          <OutcomeGlyph outcome={c.state} className="mt-px size-3.5" />
          <span className="min-w-0 break-words">
            <span className="font-medium text-foreground">{c.name}</span> <span className="text-muted-foreground">{c.detail}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}

const GLYPHS = {
  ok: { Icon: CheckCircle2, tone: "text-success", label: "servers.setup.test.passed" },
  warn: { Icon: AlertTriangle, tone: "text-warning", label: "servers.setup.test.warning" },
  fail: { Icon: XCircle, tone: "text-destructive", label: "servers.setup.test.failed" },
} as const satisfies Record<Outcome, unknown>;

/** A result's or a check's mark: passed, passed with a warning, or failed (named for readers). */
function OutcomeGlyph({ outcome, className }: { outcome: Outcome; className: string }) {
  const t = useT();
  const { Icon, tone, label } = GLYPHS[outcome];
  return <Icon role="img" aria-label={t(label)} className={`${className} shrink-0 ${tone}`} />;
}
