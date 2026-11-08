"use client";

import { CheckCircle2, Play, XCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Nav, Note, Step } from "@/features/servers/host-setup/form";
import type { ServerCheck } from "@/lib/tauri";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

export function TestStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { onDone, saved, test, runTest, v } = s;
  // A Proxmox setup made with an SSH key lists each part of it (SSH, token, key, API).
  const parts = v.provider === "proxmox" && test !== null && test !== "testing" ? test.checks : [];
  return (
    <Step icon={CheckCircle2} title={t("servers.setup.test.title")} description={t("servers.setup.test.description")}>
      {test === null || test === "testing" ? (
        <p className="flex items-center gap-2 text-[0.8125rem] text-muted-foreground">
          <Spinner className="size-4" /> {t("servers.setup.test.testing")}
        </p>
      ) : (
        <Note className="flex items-start gap-3 p-3.5">
          {test.ok ? <CheckCircle2 className="mt-px size-4 shrink-0 text-success" /> : <XCircle className="mt-px size-4 shrink-0 text-destructive" />}
          <p className="min-w-0 text-[0.8125rem] break-words text-foreground">
            {test.message}
            {test.latencyMs != null && <span className="ml-1.5 font-mono text-[0.6875rem] text-faint">{test.latencyMs} ms</span>}
          </p>
        </Note>
      )}
      {parts.length > 0 && <Checklist checks={parts} />}
      <Nav
        left={
          <Button
            variant="outline"
            onClick={() => {
              if (saved) runTest(saved.id);
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
          {c.state === "fail" ? (
            <XCircle className="mt-px size-3.5 shrink-0 text-destructive" />
          ) : (
            <CheckCircle2 className="mt-px size-3.5 shrink-0 text-success" />
          )}
          <span className="min-w-0 break-words">
            <span className="font-medium text-foreground">{c.name}</span> <span className="text-muted-foreground">{c.detail}</span>
          </span>
        </li>
      ))}
    </ul>
  );
}
