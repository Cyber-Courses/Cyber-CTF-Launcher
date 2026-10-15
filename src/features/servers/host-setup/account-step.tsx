"use client";

import { ArrowLeft, Cloud, KeyRound, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Choice, ChoiceGrid } from "@/components/ui/choice-card";
import { StatusDot } from "@/components/ui/status-pill";
import { MarkTile, Nav, Note, Step } from "@/features/servers/host-setup/form";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";

export function AccountStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { onDone, v, i, set, next, back } = s;
  return (
    <Step icon={Cloud} title={t("servers.setup.account.title")} description={t("servers.setup.account.description")}>
      <Note className="mb-4 flex items-start gap-2.5">
        <StatusDot tone="warn" className="mt-1.5" />
        <span className="text-muted-foreground">{t("servers.setup.account.billing")}</span>
      </Note>
      <ChoiceGrid>
        <Choice
          selected={!!v.useCliCreds}
          onSelect={() => {
            set("useCliCreds", true);
          }}
          mark={
            <MarkTile>
              <Terminal className="size-4" />
            </MarkTile>
          }
          title={t("servers.setup.account.cliTitle")}
          note={t("servers.setup.account.cliNote")}
        />
        <Choice
          selected={!v.useCliCreds}
          onSelect={() => {
            set("useCliCreds", false);
          }}
          mark={
            <MarkTile>
              <KeyRound className="size-4" />
            </MarkTile>
          }
          title={t("servers.setup.account.keysTitle")}
          note={t("servers.setup.account.keysNote")}
        />
      </ChoiceGrid>
      <Nav
        left={
          i > 0 ? (
            <Button variant="ghost" onClick={back}>
              <ArrowLeft className="size-4" /> {t("servers.setup.nav.back")}
            </Button>
          ) : (
            <Button variant="ghost" onClick={onDone}>
              {t("servers.setup.nav.cancel")}
            </Button>
          )
        }
        right={<Button onClick={next}>{t("servers.setup.nav.continue")}</Button>}
      />
    </Step>
  );
}
