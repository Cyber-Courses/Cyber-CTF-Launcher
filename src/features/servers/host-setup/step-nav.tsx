"use client";

import type { ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { Spinner } from "@/components/ui/spinner";
import { Nav, Note } from "@/features/servers/host-setup/form";
import { useT } from "@/lib/i18n";

/** A step's buttons: Back, then its way forward. */
export function BackNav({ onBack, children }: { onBack: () => void; children: ReactNode }) {
  const t = useT();
  return (
    <Nav
      left={
        <Button variant="ghost" onClick={onBack}>
          <ArrowLeft className="size-4" /> {t("servers.setup.nav.back")}
        </Button>
      }
      right={children}
    />
  );
}

/** On to the next step, once this one has what it needs. */
export function ContinueButton({ onClick, disabled = false }: { onClick: () => void; disabled?: boolean }) {
  const t = useT();
  return (
    <Button onClick={onClick} disabled={disabled}>
      {t("servers.setup.nav.continue")}
    </Button>
  );
}

/** Saves the host or account and tests it: busy while it saves. */
export function SaveAndTestButton({ onClick, saving, disabled = false }: { onClick: () => void; saving: boolean; disabled?: boolean }) {
  const t = useT();
  return (
    <Button onClick={onClick} disabled={saving || disabled}>
      {saving && <Spinner className="size-4" />} {t("servers.setup.nav.saveAndTest")}
    </Button>
  );
}

/** Why the last save didn't go through, when it didn't. */
export function StepError({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <Note className="mt-3 flex items-start gap-2.5">
      <StatusDot tone="fail" className="mt-1.5" />
      <span className="min-w-0 break-words text-muted-foreground">{error}</span>
    </Note>
  );
}
