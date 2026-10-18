"use client";

import { useState } from "react";
import { Segmented } from "@/components/ui/segmented";
import { getPortMode, setPortMode, type PortMode } from "@/lib/settings";
import { Row } from "@/features/settings/settings-layout";
import { useT } from "@/lib/i18n";

/** Host ports for container labs on this machine: asked at each start, or a remembered choice. */
export function LabPortsRow({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [mode, setMode] = useState<PortMode | null>(() => getPortMode());
  return (
    <Row
      title={t("settings.labPorts.title")}
      description={mode === "default" ? t("settings.labPorts.default") : mode === "random" ? t("settings.labPorts.random") : t("settings.labPorts.ask")}
      control={
        <Segmented
          label={t("settings.labPorts.title")}
          value={mode}
          onChange={(v) => {
            setPortMode(v);
            setMode(v);
            onSaved();
          }}
          options={[
            { value: null, label: t("settings.labPorts.options.ask") },
            { value: "random", label: t("settings.labPorts.options.random") },
            { value: "default", label: t("settings.labPorts.options.default") },
          ]}
        />
      }
    />
  );
}
