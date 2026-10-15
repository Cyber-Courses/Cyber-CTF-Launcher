"use client";

import { useEffect, useState } from "react";
import { Segmented } from "@/components/ui/segmented";
import { labEngine, setLabEngine, type LabEngine } from "@/lib/tauri";
import { Row } from "@/features/settings/settings-layout";
import { warn } from "@/lib/failure";
import { useT } from "@/lib/i18n";

/** The path labs on this machine run through: the launcher's own code, or the Isothrum runner
 *  (container labs on Docker, VM labs on a local hypervisor). Read once, saved on each change. */
export function LabEngineRow({ onSaved }: { onSaved: () => void }) {
  const t = useT();
  const [engine, setEngine] = useState<LabEngine | null>(null);
  useEffect(() => {
    labEngine().then(setEngine).catch(warn("reading the lab engine setting"));
  }, []);
  return (
    <Row
      title={t("settings.labEngine.title")}
      description={engine === "isothrum" ? t("settings.labEngine.isothrum") : t("settings.labEngine.launcher")}
      control={
        <Segmented
          label={t("settings.labEngine.title")}
          value={engine ?? "launcher"}
          onChange={(v) => {
            setLabEngine(v)
              .then(() => {
                setEngine(v);
                onSaved();
              })
              .catch(warn("saving the lab engine setting"));
          }}
          options={[
            { value: "launcher", label: t("settings.labEngine.options.launcher") },
            { value: "isothrum", label: t("settings.labEngine.options.isothrum") },
          ]}
        />
      }
    />
  );
}
