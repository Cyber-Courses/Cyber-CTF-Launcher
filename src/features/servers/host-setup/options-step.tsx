"use client";

import { ArrowLeft, Cloud } from "lucide-react";
import { Button } from "@/components/ui/button";
import { StatusDot } from "@/components/ui/status-pill";
import { Spinner } from "@/components/ui/spinner";
import { cn } from "@/lib/utils";
import { Field, Input, Nav, Note, Select, Step } from "@/features/servers/host-setup/form";
import type { HostSetup } from "@/features/servers/host-setup/use-host-setup";
import { useFormat, useT } from "@/lib/i18n";

export function OptionsStep({ s }: { s: HostSetup }) {
  const t = useT();
  const format = useFormat();
  const usd = (n: number) => format.number(n, { style: "currency", currency: "USD" });
  const { v, saving, error, mtdCost, set, text, connectionOk, back, saveAndTest } = s;
  return (
    <Step icon={Cloud} title={t("servers.setup.options.title")} description={t("servers.setup.options.description")}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label={t("servers.setup.options.name")}>
          <Input
            mono={false}
            {...text("name")}
            placeholder={t(`servers.setup.options.namePlaceholders.${v.provider === "azure" ? "azure" : v.provider === "gcp" ? "gcp" : "aws"}`)}
          />
        </Field>
        <Field label={t("servers.setup.options.autoStop")}>
          <Select value={String(v.autoStopHours ?? 4)} onChange={(e) => set("autoStopHours", Number(e.target.value))}>
            {/* A saved auto-stop the list lacks (72 h, say) is shown as itself, not as "1 hour". */}
            {[...new Set([1, 2, 4, 8, 12, 24, ...(v.autoStopHours ? [v.autoStopHours] : [])])]
              .sort((a, b) => a - b)
              .map((h) => (
                <option key={h} value={h}>
                  {t("servers.setup.options.hours", { count: h })}
                </option>
              ))}
            <option value={0}>{t("servers.setup.options.never")}</option>
          </Select>
        </Field>
      </div>
      <p className="mt-2 text-[0.75rem] text-muted-foreground">
        {v.provider === "aws" ? t("servers.setup.options.awsAutoStop") : t("servers.setup.options.otherAutoStop")} {t("servers.setup.options.costNote")}
      </p>
      {v.provider === "aws" && (
        <div className="mt-3">
          <Field label={t("servers.setup.options.budget")} hint={t("servers.setup.options.optional")}>
            <Input
              mono
              type="number"
              min={0}
              step={5}
              value={v.monthlyLimit ?? ""}
              onChange={(e) => set("monthlyLimit", e.target.value === "" ? null : Number(e.target.value))}
              placeholder={t("servers.setup.options.noLimit")}
            />
          </Field>
          {mtdCost != null && (
            <p className={cn("mt-1.5 text-[0.75rem]", v.monthlyLimit && mtdCost >= v.monthlyLimit ? "text-destructive" : "text-muted-foreground")}>
              {v.monthlyLimit
                ? t("servers.setup.options.spentOf", { spent: usd(mtdCost), limit: usd(v.monthlyLimit) })
                : t("servers.setup.options.spent", { spent: usd(mtdCost) })}
            </p>
          )}
        </div>
      )}
      {error && (
        <Note className="mt-3 flex items-start gap-2.5">
          <StatusDot tone="fail" className="mt-1.5" />
          <span className="min-w-0 break-words text-muted-foreground">{error}</span>
        </Note>
      )}
      <Nav
        left={
          <Button variant="ghost" onClick={back}>
            <ArrowLeft className="size-4" /> {t("servers.setup.nav.back")}
          </Button>
        }
        right={
          <Button onClick={saveAndTest} disabled={saving || !connectionOk}>
            {saving && <Spinner className="size-4" />} {t("servers.setup.nav.saveAndTest")}
          </Button>
        }
      />
    </Step>
  );
}
