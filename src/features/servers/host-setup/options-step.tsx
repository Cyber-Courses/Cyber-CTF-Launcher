"use client";

import { Cloud } from "lucide-react";
import { cn } from "@/lib/utils";
import { Field, Input, Select, Step } from "@/features/servers/host-setup/form";
import { BackNav, SaveAndTestButton, StepError } from "@/features/servers/host-setup/step-nav";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useT } from "@/lib/i18n";
import { formatUsd } from "@/lib/format";

export function OptionsStep({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, saving, error, text, connectionOk, back, saveAndTest } = s;
  const nameKey = v.provider === "azure" || v.provider === "gcp" ? v.provider : "aws";
  return (
    <Step icon={Cloud} title={t("servers.setup.options.title")} description={t("servers.setup.options.description")}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label={t("servers.setup.options.name")}>
          <Input mono={false} {...text("name")} placeholder={t(`servers.setup.options.namePlaceholders.${nameKey}`)} />
        </Field>
        <AutoStopField s={s} />
      </div>
      <p className="mt-2 text-[0.75rem] text-muted-foreground">
        {v.provider === "aws" ? t("servers.setup.options.awsAutoStop") : t("servers.setup.options.otherAutoStop")} {t("servers.setup.options.costNote")}
      </p>
      {v.provider === "aws" && <BudgetField s={s} />}
      <StepError error={error} />
      <BackNav onBack={back}>
        <SaveAndTestButton onClick={saveAndTest} saving={saving} disabled={!connectionOk} />
      </BackNav>
    </Step>
  );
}

/** How long a lab's instance lives: the usual choices, or never. */
function AutoStopField({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, set } = s;
  // A saved auto-stop the list lacks (72 h, say) is shown as itself, not as "1 hour".
  const hours = [...new Set([1, 2, 4, 8, 12, 24, ...(v.autoStopHours ? [v.autoStopHours] : [])])].sort((a, b) => a - b);
  return (
    <Field label={t("servers.setup.options.autoStop")}>
      <Select
        value={String(v.autoStopHours ?? 4)}
        onChange={(e) => {
          set("autoStopHours", Number(e.target.value));
        }}
      >
        {hours.map((h) => (
          <option key={h} value={h}>
            {t("servers.setup.options.hours", { count: h })}
          </option>
        ))}
        <option value={0}>{t("servers.setup.options.never")}</option>
      </Select>
    </Field>
  );
}

/** AWS: the monthly budget, and this month's spend so far when the CLI could read it. */
function BudgetField({ s }: { s: HostSetup }) {
  const t = useT();
  const { v, mtdCost, set } = s;
  const usd = (n: number) => formatUsd(n, t.locale);
  const limit = v.monthlyLimit ?? null;
  return (
    <div className="mt-3">
      <Field label={t("servers.setup.options.budget")} hint={t("servers.setup.options.optional")}>
        <Input
          mono
          type="number"
          min={0}
          step={5}
          value={limit ?? ""}
          onChange={(e) => {
            set("monthlyLimit", e.target.value === "" ? null : Number(e.target.value));
          }}
          placeholder={t("servers.setup.options.noLimit")}
        />
      </Field>
      {mtdCost != null && (
        <p className={cn("mt-1.5 text-[0.75rem]", limit && mtdCost >= limit ? "text-destructive" : "text-muted-foreground")}>
          {limit ? t("servers.setup.options.spentOf", { spent: usd(mtdCost), limit: usd(limit) }) : t("servers.setup.options.spent", { spent: usd(mtdCost) })}
        </p>
      )}
    </div>
  );
}
